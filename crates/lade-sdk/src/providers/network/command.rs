use anyhow::{Result, bail};
use std::path::Path;
use std::process::Command;

use super::ProviderSpec;
use super::kubeconfig::resolve_kube_context;

/// Where the provider process listens. Lade binds the user-facing port
/// itself and relays to this, except for [`ListenKind::Direct`].
#[derive(Debug, Clone, Copy)]
pub enum Listen<'a> {
    /// The provider picks a free loopback port and prints it.
    Printed,
    Tcp {
        host: &'a str,
        port: u16,
    },
    Unix(&'a Path),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListenKind {
    Printed,
    Unix,
    /// The provider binds the user-facing port itself: kubefwd maps
    /// services to their own loopback addresses.
    Direct,
}

pub fn listen_kind(spec: &ProviderSpec) -> ListenKind {
    // § jfg5ews57.c@bpp2d
    match spec {
        ProviderSpec::Kubectl { .. }
        | ProviderSpec::TshKubeCluster { .. }
        | ProviderSpec::TshApp { .. } => ListenKind::Printed,
        ProviderSpec::Ssh { .. } => ListenKind::Unix,
        ProviderSpec::Kubefwd { .. } => ListenKind::Direct,
    }
}

/// `kubectl port-forward` arguments: `:remote` lets kubectl pick the local
/// port, so no other process can race for it.
fn port_forward_args(listen: Listen<'_>, remote_port: &str) -> Result<[String; 3]> {
    // § jfg5ews57.a@9bqan
    let Listen::Printed = listen else {
        bail!("kubectl port-forward picks its own port");
    };
    Ok([
        format!(":{remote_port}"),
        "--address".to_string(),
        "127.0.0.1".to_string(),
    ])
}

/// The kube context a kubectl or kubefwd URI points at, read with this
/// process's env (`KUBECONFIG`). `None` for providers without one.
pub fn kube_context(spec: &ProviderSpec) -> Result<Option<String>> {
    match spec {
        ProviderSpec::Kubectl {
            cluster_endpoint,
            context_selector,
            ..
        }
        | ProviderSpec::Kubefwd {
            cluster_endpoint,
            context_selector,
            ..
        } => resolve_kube_context(cluster_endpoint, Some(context_selector.as_str())).map(Some),
        ProviderSpec::TshKubeCluster { .. }
        | ProviderSpec::TshApp { .. }
        | ProviderSpec::Ssh { .. } => Ok(None),
    }
}

fn required_context(context: Option<&str>) -> Result<&str> {
    context.ok_or_else(|| anyhow::anyhow!("kube context was not resolved"))
}

/// `context` is [`kube_context`] for the same spec: the caller resolves it
/// where the user's env lives, the command may run elsewhere.
pub fn build_command(
    spec: &ProviderSpec,
    context: Option<&str>,
    listen: Listen<'_>,
) -> Result<Command> {
    match spec {
        ProviderSpec::Kubectl {
            namespace,
            kind,
            name,
            remote_port,
            pod_running_timeout,
            ..
        } => {
            let context = required_context(context)?;
            let mut cmd = Command::new("kubectl");
            cmd.arg("--context")
                .arg(context)
                .arg("-n")
                .arg(namespace)
                .arg("port-forward")
                .arg(format!("{kind}/{name}"))
                .args(port_forward_args(listen, remote_port)?);
            if let Some(timeout) = pod_running_timeout {
                cmd.arg("--pod-running-timeout").arg(timeout);
            }
            Ok(cmd)
        }
        ProviderSpec::Kubefwd {
            namespace,
            kind,
            name,
            service_port,
            domain,
            selector,
            ..
        } => {
            if kind != "service" {
                bail!("kubefwd currently supports only resource kind 'service'");
            }
            let Listen::Tcp {
                port: local_port, ..
            } = listen
            else {
                bail!("kubefwd binds its own local port");
            };
            let context = required_context(context)?;
            let mut cmd = Command::new("kubefwd");
            cmd.arg("svc")
                .arg("-n")
                .arg(namespace)
                .arg("-x")
                .arg(context)
                .arg("-m")
                .arg(format!("{service_port}:{local_port}"))
                .arg("-f")
                .arg(format!("metadata.name={name}"));
            if let Some(domain) = domain {
                cmd.arg("-d").arg(domain);
            }
            if let Some(selector) = selector {
                cmd.arg("-l").arg(selector);
            }
            Ok(cmd)
        }
        ProviderSpec::TshKubeCluster {
            teleport_proxy,
            namespace,
            kind,
            name,
            remote_port,
            ..
        } => {
            let mut cmd = Command::new("tsh");
            cmd.arg(format!("--proxy={teleport_proxy}"))
                .arg("kubectl")
                .arg("-n")
                .arg(namespace)
                .arg("port-forward")
                .arg(format!("{kind}/{name}"))
                .args(port_forward_args(listen, &remote_port.to_string())?);
            Ok(cmd)
        }
        ProviderSpec::TshApp {
            teleport_proxy,
            app_name,
            target_port,
        } => {
            // Without a local port, tsh binds localhost:0 and prints
            // `Proxying connections to APP on HOST:PORT`.
            // § jfg5ews57.a@npefe
            let Listen::Printed = listen else {
                bail!("tsh app proxy picks its own port");
            };
            let mut cmd = Command::new("tsh");
            cmd.arg(format!("--proxy={teleport_proxy}"))
                .arg("proxy")
                .arg("app");
            if let Some(target_port) = target_port {
                cmd.arg(format!("--port=0:{target_port}"));
            }
            cmd.arg(app_name);
            Ok(cmd)
        }
        ProviderSpec::Ssh {
            jump_host,
            jump_port,
            remote_host,
            remote_port,
        } => {
            // § jfg5ews57.b@t7e5y
            let Listen::Unix(socket) = listen else {
                bail!("ssh forwards listen on a Unix socket");
            };
            let mut cmd = Command::new("ssh");
            // ControlPath=none: a shared master from ~/.ssh/config would own
            // the forward, so killing this ssh would not close it.
            // ServerAlive*: a dead link (sleep, network change) makes ssh
            // exit and get restarted instead of hanging.
            // StreamLocalBindUnlink: a restart reuses the socket path the
            // previous ssh left behind.
            cmd.arg("-N")
                .args(["-o", "ExitOnForwardFailure=yes"])
                // § vdbmtzqtb.c@w2cmn
                .args(["-o", "ControlPath=none"])
                // § end vdbmtzqtb.c
                .args(["-o", "StreamLocalBindUnlink=yes"])
                // § bkmpmswkm.d@bsf63
                .args(["-o", "ServerAliveInterval=15"])
                .args(["-o", "ServerAliveCountMax=3"])
                // § end bkmpmswkm.d
                .arg("-L")
                .arg(format!("{}:{remote_host}:{remote_port}", socket.display()));
            if let Some(port) = jump_port {
                cmd.arg("-p").arg(port.to_string());
            }
            cmd.arg(jump_host);
            Ok(cmd)
        }
    }
}

const MISSING_SSH: &str = "ssh is not on PATH. Lade uses the OpenSSH client for ssh:// tunnels, not a mise pin. Install OpenSSH for this OS, then retry. https://www.openssh.com/";

/// Checks what a provider needs before it starts. A `tsh://` kube_cluster
/// also needs [`tsh_kube_login`], which its caller runs where the terminal
/// is.
pub fn ensure_provider_preflight(spec: &ProviderSpec) -> Result<()> {
    if matches!(spec, ProviderSpec::Ssh { .. }) && !ssh_on_path() {
        bail!("{MISSING_SSH}");
    }
    Ok(())
}

const LOGIN_POLL: std::time::Duration = std::time::Duration::from_millis(50);

/// `tsh kube login`, which may prompt on the caller's terminal. Kills the
/// login and fails once `stopped` returns true.
pub fn tsh_kube_login(proxy: &str, cluster: &str, stopped: impl Fn() -> bool) -> Result<()> {
    let mut login = Command::new("tsh");
    login
        .arg(format!("--proxy={proxy}"))
        .arg("kube")
        .arg("login")
        .arg(cluster);
    run_login(login, cluster, stopped)
}

fn run_login(mut login: Command, cluster: &str, stopped: impl Fn() -> bool) -> Result<()> {
    // § n6qm5smv5.a@5c47t
    let mut child = login.spawn()?;
    loop {
        match child.try_wait()? {
            Some(status) if status.success() => return Ok(()),
            Some(_) => bail!("tsh kube login failed for cluster '{cluster}'"),
            None => {
                if stopped() {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("stopped while logging in to the tunnel provider");
                }
                std::thread::sleep(LOGIN_POLL);
            }
        }
    }
}

fn ssh_on_path() -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path)
        .any(|dir| dir.join("ssh").is_file() || dir.join("ssh.exe").is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_app_proxy_command() {
        let spec = ProviderSpec::TshApp {
            teleport_proxy: "teleport.example.com:443".to_string(),
            app_name: "example-app".to_string(),
            target_port: None,
        };
        let command = build_command(&spec, None, Listen::Printed).expect("command");
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(
            args,
            [
                "--proxy=teleport.example.com:443",
                "proxy",
                "app",
                "example-app"
            ]
        );
    }

    #[test]
    fn ssh_forwards_to_a_unix_socket_and_keeps_the_config_port() {
        let spec = ProviderSpec::Ssh {
            jump_host: "jump.example.com".to_string(),
            jump_port: None,
            remote_host: "db.internal".to_string(),
            remote_port: 5432,
        };
        let command =
            build_command(&spec, None, Listen::Unix(Path::new("/tmp/t/s.sock"))).expect("command");
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert!(args.contains(&"/tmp/t/s.sock:db.internal:5432".to_string()));
        assert!(args.contains(&"ServerAliveInterval=15".to_string()));
        assert!(args.contains(&"ControlPath=none".to_string()));
        assert!(!args.contains(&"-p".to_string()));
        assert_eq!(args.last().map(String::as_str), Some("jump.example.com"));
    }

    #[test]
    fn builds_app_proxy_command_with_target_port() {
        let spec = ProviderSpec::TshApp {
            teleport_proxy: "teleport.example.com:443".to_string(),
            app_name: "example-app".to_string(),
            target_port: Some(3000),
        };
        let command = build_command(&spec, None, Listen::Printed).expect("command");
        let args = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(
            args,
            [
                "--proxy=teleport.example.com:443",
                "proxy",
                "app",
                "--port=0:3000",
                "example-app"
            ]
        );
    }

    #[test]
    fn missing_ssh_copy_points_at_openssh() {
        assert!(MISSING_SSH.contains("OpenSSH"));
        assert!(MISSING_SSH.contains("https://www.openssh.com/"));
        assert!(MISSING_SSH.contains("not a mise pin"));
    }

    #[test]
    fn tsh_kube_login_stops_when_asked() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let tsh = dir.path().join("tsh");
        let pid_file = dir.path().join("pid");
        std::fs::write(
            &tsh,
            format!(
                "#!/bin/sh\necho $$ > {}\nexec sleep 60\n",
                pid_file.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&tsh, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = std::time::Instant::now();
        let error = run_login(Command::new(&tsh), "prd", || pid_file.exists()).unwrap_err();
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
        assert!(error.to_string().contains("stopped"), "{error:#}");
        let pid: libc::pid_t = std::fs::read_to_string(&pid_file)
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert_ne!(unsafe { libc::kill(pid, 0) }, 0, "the login still runs");
    }

    #[test]
    fn kubectl_lets_the_provider_pick_its_port() {
        let kubectl = ProviderSpec::Kubectl {
            cluster_endpoint: "kube.example.com:443".to_string(),
            context_selector: "acme".to_string(),
            namespace: "default".to_string(),
            kind: "service".to_string(),
            name: "db".to_string(),
            remote_port: "5432".to_string(),
            pod_running_timeout: None,
        };
        let tsh = ProviderSpec::TshKubeCluster {
            teleport_proxy: "teleport.example.com:443".to_string(),
            kube_cluster: "prd".to_string(),
            namespace: "default".to_string(),
            kind: "service".to_string(),
            name: "db".to_string(),
            remote_port: 5432,
        };
        for spec in [kubectl, tsh] {
            let command = build_command(&spec, Some("acme"), Listen::Printed).expect("command");
            let args = command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>();
            assert!(
                args.ends_with(&[
                    ":5432".to_string(),
                    "--address".to_string(),
                    "127.0.0.1".to_string()
                ]),
                "{args:?}"
            );
            let fixed = Listen::Tcp {
                host: "127.0.0.1",
                port: 15432,
            };
            assert!(build_command(&spec, Some("acme"), fixed).is_err());
        }
    }
}
