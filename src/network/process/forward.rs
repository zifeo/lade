use anyhow::{Result, anyhow, bail};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::{READINESS_POLL_INTERVAL, configure_child_process, tcp_connects};

const STARTUP_TIMEOUT: Duration = Duration::from_secs(60);
const INITIAL_RESTART_BACKOFF: Duration = Duration::from_millis(250);
const MAX_RESTART_BACKOFF: Duration = Duration::from_secs(5);

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
