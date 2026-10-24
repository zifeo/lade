use anyhow::{Result, bail};
use lade_sdk::network::{Listen, ListenKind, build_command, listen_kind};
use nix::errno::Errno;
use nix::sys::signal::kill;
use nix::unistd::Pid;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use super::guard::GUARD_VERB;
use super::labels::provider_label;
use super::outer::Outer;
use super::parse::parse_binding;
use super::process::{Direct, Endpoint, Inner, RunningForward, Stop};
use super::relay::Relay;
use crate::config::NetworkBinding;

// macOS `sun_path` is 104 bytes.
const MAX_SOCKET_PATH: usize = 103;

static SOCKET_COUNTER: AtomicU64 = AtomicU64::new(0);

/// What a tunnel runner needs. Built where the user's env lives: the kube
/// context comes from their kubeconfig, PATH holds the pinned provider.
#[derive(Debug, Clone)]
pub(crate) struct TunnelRequest {
    pub(crate) key: String,
    pub(crate) uri: String,
    pub(crate) context: Option<String>,
    /// `None`: inherit this process's env.
    pub(crate) env: Option<Vec<(OsString, OsString)>>,
}

/// What frees the user-facing port on [`Tunnel::release_port`].
#[derive(Debug)]
enum Front {
    Relay(Relay),
    Direct,
}

/// Fields drop in order: stop accepting, stop the provider, remove its socket.
#[derive(Debug)]
pub(crate) struct Tunnel {
    front: Front,
    _forward: RunningForward,
    _socket: Option<SocketFile>,
}

/// What still runs once [`Tunnel::release_port`] freed the port.
#[derive(Debug)]
pub(crate) struct Provider {
    _forward: Option<RunningForward>,
    _socket: Option<SocketFile>,
}

impl Tunnel {
    /// Frees the local port now. A relayed provider stops later. Direct
    /// listens on that port, so it is stopped here.
    pub(crate) fn release_port(self) -> Provider {
        match self.front {
            Front::Relay(relay) => {
                drop(relay);
                Provider {
                    _forward: Some(self._forward),
                    _socket: self._socket,
                }
            }
            Front::Direct => {
                drop(self._forward);
                Provider {
                    _forward: None,
                    _socket: self._socket,
                }
            }
        }
    }
}

/// Blocks until the provider is ready. Returns its pid.
pub(crate) fn start_tunnel(
    outer: Outer,
    request: &TunnelRequest,
    stop: Stop,
) -> Result<(Tunnel, u32)> {
    let parsed = parse_binding(&NetworkBinding {
        key: request.key.clone(),
        uri: request.uri.clone(),
        ttl: None,
    })?;
    let mut socket = None;
    let (inner, outer) = match listen_kind(&parsed.spec) {
        ListenKind::Printed => (Inner::Printed, Some(outer)),
        ListenKind::Unix => {
            let file = SocketFile::create()?;
            let endpoint = Endpoint::Unix(file.0.clone());
            socket = Some(file);
            (Inner::Fixed(endpoint), Some(outer))
        }
        ListenKind::Direct => {
            let host = outer.host.clone();
            let port = outer.port;
            (Inner::Direct(Direct::holding(outer, host, port)), None)
        }
    };
    let command = build_command(&parsed.spec, request.context.as_deref(), listen(&inner))?;
    let template = CommandTemplate::new(&command, request.env.clone())?;
    let (forward, pid) = RunningForward::supervise(
        provider_label(&parsed.spec).to_string(),
        inner,
        stop,
        move || Ok(template.command()),
    )?;
    let front = match outer {
        Some(outer) => Front::Relay(Relay::start(outer, forward.endpoint())?),
        None => Front::Direct,
    };
    Ok((
        Tunnel {
            front,
            _forward: forward,
            _socket: socket,
        },
        pid,
    ))
}

fn listen(inner: &Inner) -> Listen<'_> {
    match inner {
        Inner::Printed => Listen::Printed,
        Inner::Fixed(Endpoint::Tcp { host, port }) | Inner::Direct(Direct { host, port, .. }) => {
            Listen::Tcp { host, port: *port }
        }
        Inner::Fixed(Endpoint::Unix(path)) => Listen::Unix(path),
    }
}

#[derive(Clone)]
struct CommandTemplate {
    /// This binary, run as `lade tunnel-guard`. `None` in unit tests, whose
    /// binary is not Lade.
    guard: Option<PathBuf>,
    program: OsString,
    args: Vec<OsString>,
    env: Option<Vec<(OsString, OsString)>>,
}

impl CommandTemplate {
    fn new(command: &Command, env: Option<Vec<(OsString, OsString)>>) -> Result<Self> {
        Ok(Self {
            guard: if cfg!(test) {
                None
            } else {
                Some(std::env::current_exe()?)
            },
            program: command.get_program().to_owned(),
            args: command.get_args().map(OsString::from).collect(),
            env,
        })
    }

    fn command(&self) -> Command {
        let mut command = match &self.guard {
            Some(guard) => {
                let mut command = Command::new(guard);
                command.args([GUARD_VERB, "--"]).arg(&self.program);
                command
            }
            None => Command::new(&self.program),
        };
        command.args(&self.args);
        if let Some(env) = &self.env {
            command.env_clear().envs(env.iter().map(|(k, v)| (k, v)));
        }
        command
    }
}

/// `<pid>-<n>.sock`, named after the runner so a later one can sweep the
/// sockets of a runner that died without dropping its tunnels.
#[derive(Debug)]
struct SocketFile(PathBuf);

impl SocketFile {
    fn create() -> Result<Self> {
        let dir = crate::cache::tunnel_sockets();
        fs::create_dir_all(&dir)?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        sweep_stale_sockets(&dir);
        let n = SOCKET_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("{}-{n}.sock", std::process::id()));
        if path.as_os_str().len() > MAX_SOCKET_PATH {
            bail!(
                "tunnel socket path is too long: {}. Set LADE_CACHE_DIR to a shorter directory.",
                path.display()
            );
        }
        // A crashed runner with this pid may have left an ssh listening
        // here, and readiness would connect to it.
        match fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
        Ok(Self(path))
    }
}

impl Drop for SocketFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn sweep_stale_sockets(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|name| name.split_once('-'))
            .and_then(|(pid, _)| pid.parse::<i32>().ok())
        else {
            continue;
        };
        if kill(Pid::from_raw(pid), None) == Err(Errno::ESRCH) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn sweep_removes_sockets_of_dead_runners_only() {
        let dir = tempfile::tempdir().unwrap();
        let mut child = Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let dead = child.id();
        child.wait().unwrap();
        let stale = dir.path().join(format!("{dead}-0.sock"));
        let live = dir.path().join(format!("{}-0.sock", std::process::id()));
        fs::write(&stale, "").unwrap();
        fs::write(&live, "").unwrap();
        sweep_stale_sockets(dir.path());
        assert!(!stale.exists());
        assert!(live.exists());
    }

    #[test]
    fn socket_file_is_removed_on_drop() {
        let cache = tempfile::tempdir().unwrap();
        temp_env::with_var("LADE_CACHE_DIR", Some(cache.path()), || {
            let file = SocketFile::create().unwrap();
            let path = file.0.clone();
            fs::write(&path, "").unwrap();
            drop(file);
            assert!(!path.exists());
        });
    }

    fn write_script(dir: &Path, name: &str, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn kubefwd_request(dir: &Path) -> TunnelRequest {
        let path = format!("{}:/bin:/usr/bin", dir.display());
        TunnelRequest {
            key: "db".to_string(),
            uri: "kubefwd://kube.example.com:443/acme/default/service/db/5432".to_string(),
            context: Some("acme".to_string()),
            env: Some(vec![(OsString::from("PATH"), OsString::from(path))]),
        }
    }

    #[test]
    fn direct_does_not_ready_when_a_stranger_binds() {
        let dir = tempfile::tempdir().unwrap();
        write_script(dir.path(), "kubefwd", "exec sleep 60");
        let outer = Outer::claim("127.0.0.1", None).unwrap();
        let port = outer.port;
        std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while std::time::Instant::now() < deadline {
                if let Ok(listener) = std::net::TcpListener::bind(("127.0.0.1", port)) {
                    std::thread::park();
                    drop(listener);
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let err = start_tunnel(outer, &kubefwd_request(dir.path()), Stop::default()).unwrap_err();
        assert!(
            err.to_string().contains("another process is listening"),
            "{err:#}"
        );
    }

    #[test]
    fn release_port_frees_a_direct_port() {
        let dir = tempfile::tempdir().unwrap();
        write_script(
            dir.path(),
            "kubefwd",
            r#"port=0
while [ "$#" -gt 0 ]; do
  if [ "$1" = "-m" ]; then
    port="${2##*:}"
    shift 2
    continue
  fi
  shift
done
exec /usr/bin/python3 -c 'import socket,sys,time
s=socket.socket()
s.bind(("127.0.0.1", int(sys.argv[1])))
s.listen()
time.sleep(60)
' "$port""#,
        );
        let freed = (0..8).any(|_| {
            let outer = Outer::claim("127.0.0.1", None).unwrap();
            let port = outer.port;
            let (tunnel, _) = start_tunnel(outer, &kubefwd_request(dir.path()), Stop::default())
                .expect("direct start");
            drop(tunnel.release_port());
            Outer::claim("127.0.0.1", Some(port)).is_ok()
        });
        assert!(freed);
    }
}
