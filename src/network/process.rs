use anyhow::{Result, bail};
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Delay between readiness checks while waiting for a forwarded port to
/// accept connections. Kept short for snappier detection; the dominant cost
/// is the provider CLI starting up, not this poll loop.
const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(30);
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
    name: String,
    pub(crate) child: Child,
    logs: Arc<Mutex<Vec<u8>>>,
    stdout_join: Option<JoinHandle<()>>,
    stderr_join: Option<JoinHandle<()>>,
}

impl RunningForward {
    pub(crate) fn spawn(name: &str, mut command: Command) -> Result<Self> {
        configure_child_process(&mut command);
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let logs = Arc::new(Mutex::new(Vec::new()));
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let stdout_join = stdout.map(|pipe| spawn_pipe_reader(pipe, Arc::clone(&logs)));
        let stderr_join = stderr.map(|pipe| spawn_pipe_reader(pipe, Arc::clone(&logs)));
        Ok(Self {
            name: name.to_string(),
            child,
            logs,
            stdout_join,
            stderr_join,
        })
    }

    pub(crate) fn wait_ready(&mut self, host: &str, port: u16, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(status) = self.child.try_wait()? {
                bail!(
                    "{} exited before becoming ready (status: {}): {}",
                    self.name,
                    status,
                    self.logs_text()
                );
            }
            if tcp_connects(host, port, Duration::from_millis(200)) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "{} did not become ready on {}:{} before timeout: {}",
                    self.name,
                    host,
                    port,
                    self.logs_text()
                );
            }
            std::thread::sleep(READINESS_POLL_INTERVAL);
        }
    }

    fn logs_text(&self) -> String {
        String::from_utf8_lossy(&self.logs.lock().expect("logs mutex")).into_owned()
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
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
        if let Some(handle) = self.stdout_join.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_join.take() {
            let _ = handle.join();
        }
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
}
