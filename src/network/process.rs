use anyhow::{Result, bail};
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use std::io::Read;
use std::net::{TcpStream, ToSocketAddrs};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// Delay between readiness checks while waiting for a forwarded port to
/// accept connections. Kept short for snappier detection; the dominant cost
/// is the provider CLI starting up, not this poll loop.
const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(30);

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
