use anyhow::{Result, anyhow, bail};
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Delay between readiness checks while waiting for a forwarded port to
/// accept connections. Kept short for snappier detection; the dominant cost
/// is the provider CLI starting up, not this poll loop.
const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(30);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const INITIAL_RESTART_BACKOFF: Duration = Duration::from_millis(250);
const MAX_RESTART_BACKOFF: Duration = Duration::from_secs(5);
static LOG_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) struct ChildOutputFiles {
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

impl ChildOutputFiles {
    pub(crate) fn capture(command: &mut Command) -> Result<Self> {
        let (stdout_path, stdout) = create_log_file("stdout")?;
        let (stderr_path, stderr) = create_log_file("stderr")?;
        command
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        Ok(Self {
            stdout_path,
            stderr_path,
        })
    }

    pub(crate) fn read_text(&self) -> String {
        let stdout = fs::read_to_string(&self.stdout_path).unwrap_or_default();
        let stderr = fs::read_to_string(&self.stderr_path).unwrap_or_default();
        let mut parts = Vec::new();
        if !stdout.trim().is_empty() {
            parts.push(format!("stdout:\n{}", dedupe_lines(&stdout)));
        }
        if !stderr.trim().is_empty() {
            parts.push(format!("stderr:\n{}", dedupe_lines(&stderr)));
        }
        parts.join("\n")
    }

    pub(crate) fn cleanup(&self) {
        let _ = fs::remove_file(&self.stdout_path);
        let _ = fs::remove_file(&self.stderr_path);
    }
}

#[derive(Debug)]
pub(crate) struct RunningForward {
    shutdown: Arc<AtomicBool>,
    child_pid: Arc<AtomicI32>,
    shutdown_tx: mpsc::Sender<()>,
    supervisor: Option<JoinHandle<()>>,
}

struct SupervisorConfig {
    name: String,
    host: String,
    port: u16,
}

impl RunningForward {
    pub(crate) fn supervise<F>(
        name: String,
        host: String,
        port: u16,
        command: F,
    ) -> Result<(Self, u32)>
    where
        F: Fn() -> Result<Command> + Send + 'static,
    {
        let shutdown = Arc::new(AtomicBool::new(false));
        let child_pid = Arc::new(AtomicI32::new(0));
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let supervisor_shutdown = Arc::clone(&shutdown);
        let supervisor_pid = Arc::clone(&child_pid);
        let supervisor = std::thread::spawn(move || {
            supervise(
                SupervisorConfig { name, host, port },
                command,
                supervisor_shutdown,
                supervisor_pid,
                shutdown_rx,
                ready_tx,
            )
        });
        let pid = ready_rx
            .recv()
            .map_err(|_| anyhow!("network provider supervisor stopped before readiness"))??;
        Ok((
            Self {
                shutdown,
                child_pid,
                shutdown_tx,
                supervisor: Some(supervisor),
            },
            pid,
        ))
    }
}

pub(crate) fn configure_child_process(command: &mut Command) {
    #[cfg(target_family = "unix")]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                nix::unistd::setsid().map_err(|e| std::io::Error::other(e.to_string()))?;
                Ok(())
            });
        }
    }
}

impl Drop for RunningForward {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Release);
        let _ = self.shutdown_tx.send(());
        let pid = self.child_pid.load(Ordering::Acquire);
        if pid > 0 {
            let _ = killpg(Pid::from_raw(pid), Signal::SIGTERM);
        }
        if let Some(handle) = self.supervisor.take() {
            let _ = handle.join();
        }
    }
}

fn supervise<F>(
    config: SupervisorConfig,
    command: F,
    shutdown: Arc<AtomicBool>,
    child_pid: Arc<AtomicI32>,
    shutdown_rx: mpsc::Receiver<()>,
    ready_tx: mpsc::SyncSender<Result<u32>>,
) where
    F: Fn() -> Result<Command>,
{
    let startup_deadline = Instant::now() + STARTUP_TIMEOUT;
    let mut ready_sent = false;
    let mut backoff = INITIAL_RESTART_BACKOFF;
    loop {
        if shutdown.load(Ordering::Acquire) {
            if !ready_sent {
                let _ = ready_tx.send(Err(anyhow!("network provider stopped before readiness")));
            }
            return;
        }
        let mut child = match command().and_then(spawn_forward) {
            Ok(child) => child,
            Err(error) => {
                if !ready_sent && Instant::now() >= startup_deadline {
                    let _ = ready_tx.send(Err(error));
                    return;
                }
                log::warn!("{} failed to start: {error}", config.name);
                if !wait_restart(&shutdown_rx, backoff) {
                    return;
                }
                backoff = (backoff * 2).min(MAX_RESTART_BACKOFF);
                continue;
            }
        };
        match wait_forward_ready(
            &mut child,
            &config.host,
            config.port,
            startup_deadline,
            &shutdown_rx,
        ) {
            Ok(()) => {
                child_pid.store(child.child.id() as i32, Ordering::Release);
                if !ready_sent {
                    let _ = ready_tx.send(Ok(child.child.id()));
                    ready_sent = true;
                }
                backoff = INITIAL_RESTART_BACKOFF;
                let status = child.child.wait();
                child_pid.store(0, Ordering::Release);
                child.join_readers();
                if shutdown.load(Ordering::Acquire) {
                    return;
                }
                log::warn!(
                    "{} exited after readiness (status: {}): {}",
                    config.name,
                    status.map_or_else(|error| error.to_string(), |status| status.to_string()),
                    child.logs_text()
                );
            }
            Err(error) => {
                stop_child(&mut child.child);
                child_pid.store(0, Ordering::Release);
                child.join_readers();
                if shutdown.load(Ordering::Acquire) {
                    return;
                }
                if !ready_sent && Instant::now() >= startup_deadline {
                    let _ = ready_tx.send(Err(error));
                    return;
                }
                log::warn!("{} failed before readiness: {error}", config.name);
            }
        }
        if !wait_restart(&shutdown_rx, backoff) {
            return;
        }
        backoff = (backoff * 2).min(MAX_RESTART_BACKOFF);
    }
}

fn wait_restart(shutdown_rx: &mpsc::Receiver<()>, delay: Duration) -> bool {
    shutdown_rx.recv_timeout(delay).is_err()
}

struct ChildForward {
    child: Child,
    logs: Arc<Mutex<Vec<u8>>>,
    stdout_join: Option<JoinHandle<()>>,
    stderr_join: Option<JoinHandle<()>>,
}

fn spawn_forward(mut command: Command) -> Result<ChildForward> {
    configure_child_process(&mut command);
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let logs = Arc::new(Mutex::new(Vec::new()));
    let stdout_join = child
        .stdout
        .take()
        .map(|pipe| spawn_pipe_reader(pipe, Arc::clone(&logs)));
    let stderr_join = child
        .stderr
        .take()
        .map(|pipe| spawn_pipe_reader(pipe, Arc::clone(&logs)));
    Ok(ChildForward {
        child,
        logs,
        stdout_join,
        stderr_join,
    })
}

impl ChildForward {
    fn logs_text(&self) -> String {
        String::from_utf8_lossy(&self.logs.lock().expect("logs mutex")).into_owned()
    }

    fn join_readers(&mut self) {
        if let Some(handle) = self.stdout_join.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_join.take() {
            let _ = handle.join();
        }
    }
}

fn wait_forward_ready(
    child: &mut ChildForward,
    host: &str,
    port: u16,
    deadline: Instant,
    shutdown_rx: &mpsc::Receiver<()>,
) -> Result<()> {
    loop {
        if let Some(status) = child.child.try_wait()? {
            bail!(
                "process exited before becoming ready (status: {status}): {}",
                child.logs_text()
            );
        }
        if tcp_connects(host, port, Duration::from_millis(200)) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!(
                "timeout waiting for readiness on {host}:{port}: {}",
                child.logs_text()
            );
        }
        if shutdown_rx.recv_timeout(READINESS_POLL_INTERVAL).is_ok() {
            bail!("network provider stopped before readiness");
        }
    }
}

fn stop_child(child: &mut Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        let _ = killpg(Pid::from_raw(pid), Signal::SIGTERM);
    }
    if child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

fn spawn_pipe_reader<R: Read + Send + 'static>(
    mut reader: R,
    logs: Arc<Mutex<Vec<u8>>>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if reader.read_to_end(&mut buf).is_ok() && !buf.is_empty() {
            logs.lock().expect("logs mutex").extend(buf);
        }
    })
}

fn create_log_file(stream: &str) -> Result<(PathBuf, File)> {
    for _ in 0..16 {
        let idx = LOG_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "lade-network-{}-{idx}-{stream}.log",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    bail!("could not create network provider log file")
}

fn dedupe_lines(raw: &str) -> String {
    let mut seen = HashSet::new();
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim_end();
            if trimmed.is_empty() || !seen.insert(dedupe_key(trimmed)) {
                None
            } else {
                Some(trimmed)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn dedupe_key(line: &str) -> String {
    if let Some((_, rest)) = line.split_once(" memcache.go:") {
        return format!("memcache.go:{rest}");
    }
    line.to_string()
}

fn tcp_connects(host: &str, port: u16, timeout: Duration) -> bool {
    let target = format!("{host}:{port}");
    let Ok(addrs) = target.to_socket_addrs() else {
        return false;
    };
    addrs
        .into_iter()
        .any(|addr| TcpStream::connect_timeout(&addr, timeout).is_ok())
}

pub(crate) fn wait_child_ready(
    child: &mut Child,
    host: &str,
    port: u16,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            bail!("process exited before becoming ready (status: {status})");
        }
        if tcp_connects(host, port, Duration::from_millis(200)) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("timeout waiting for readiness on {host}:{port}");
        }
        std::thread::sleep(READINESS_POLL_INTERVAL);
    }
}

pub fn stop_network_pids(raw: &str) {
    let pids = raw
        .split(',')
        .filter_map(|part| part.trim().parse::<i32>().ok())
        .filter_map(|pid| u32::try_from(pid).ok())
        .collect::<Vec<_>>();
    stop_network_pids_list(&pids);
}

pub fn stop_network_pids_list(pids: &[u32]) {
    for pid in pids.iter().filter_map(|pid| i32::try_from(*pid).ok()) {
        match killpg(Pid::from_raw(pid), Signal::SIGTERM) {
            Ok(_) | Err(Errno::ESRCH) => {}
            Err(_) => {}
        }
        match kill(Pid::from_raw(pid), Signal::SIGTERM) {
            Ok(_) | Err(Errno::ESRCH) => {}
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn child_output_files_capture_stdout_and_stderr() {
        let mut command = Command::new("sh");
        command.args(["-c", "printf 'out\nout'; printf 'err\nerr' >&2"]);
        let logs = ChildOutputFiles::capture(&mut command).unwrap();
        let status = command.spawn().unwrap().wait().unwrap();
        assert!(status.success());
        let text = logs.read_text();
        logs.cleanup();
        assert!(text.contains("stdout:\nout"));
        assert!(text.contains("stderr:\nerr"));
        assert!(!text.contains("out\nout"));
        assert!(!text.contains("err\nerr"));
    }

    #[test]
    fn dedupe_lines_collapses_kubernetes_memcache_retries() {
        let raw = concat!(
            "E0705 00:01:52.834950   86438 memcache.go:265] \"Unhandled Error\" err=\"same\"\n",
            "E0705 00:01:52.894498   86438 memcache.go:265] \"Unhandled Error\" err=\"same\"\n",
            "Unable to connect to the server: same\n",
        );

        let text = dedupe_lines(raw);
        assert_eq!(text.matches("memcache.go").count(), 1);
        assert!(text.contains("Unable to connect to the server: same"));
    }

    #[test]
    #[cfg(unix)]
    fn dropping_supervisor_terminates_the_provider_process_group() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (forward, pid) = RunningForward::supervise(
            "test forward".to_string(),
            "127.0.0.1".to_string(),
            address.port(),
            || {
                let mut command = Command::new("sh");
                command.args(["-c", "while :; do :; done"]);
                Ok(command)
            },
        )
        .unwrap();
        drop(forward);
        assert_eq!(kill(Pid::from_raw(pid as i32), None), Err(Errno::ESRCH));
    }

    #[test]
    #[cfg(unix)]
    fn supervisor_retries_a_startup_failure() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let command_attempts = Arc::clone(&attempts);
        let (forward, _) = RunningForward::supervise(
            "test forward".to_string(),
            "127.0.0.1".to_string(),
            address.port(),
            move || {
                let attempt = command_attempts.fetch_add(1, Ordering::SeqCst);
                if attempt == 0 {
                    return Err(anyhow!("simulated startup failure"));
                }
                let mut command = Command::new("sh");
                command.args(["-c", "while :; do :; done"]);
                Ok(command)
            },
        )
        .unwrap();
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        drop(forward);
    }
}
