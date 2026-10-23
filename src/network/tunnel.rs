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
use super::parse::parse_binding;
use super::process::{Endpoint, Inner, RunningForward, Stop};
use super::relay::{Outer, Relay};
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

/// Fields drop in order: stop accepting, stop the provider, remove its socket.
#[derive(Debug)]
pub(crate) struct Tunnel {
    _relay: Option<Relay>,
    _forward: RunningForward,
    _socket: Option<SocketFile>,
}

/// What still runs once [`Tunnel::release_port`] freed the port.
#[derive(Debug)]
pub(crate) struct Provider {
    _forward: RunningForward,
    _socket: Option<SocketFile>,
}

impl Tunnel {
    /// Frees the local port now. Stopping the provider takes longer.
    pub(crate) fn release_port(self) -> Provider {
        let Tunnel {
            _relay,
            _forward,
            _socket,
        } = self;
        drop(_relay);
        Provider { _forward, _socket }
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
            let endpoint = Endpoint::Tcp {
                host: outer.host.clone(),
                port: outer.port,
            };
            drop(outer);
            (Inner::Fixed(endpoint), None)
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
    let relay = match outer {
        Some(outer) => Some(Relay::start(outer, forward.endpoint())?),
        None => None,
    };
    Ok((
        Tunnel {
            _relay: relay,
            _forward: forward,
            _socket: socket,
        },
        pid,
    ))
}

fn listen(inner: &Inner) -> Listen<'_> {
    match inner {
        Inner::Printed => Listen::Printed,
        Inner::Fixed(Endpoint::Tcp { host, port }) => Listen::Tcp { host, port: *port },
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
}
