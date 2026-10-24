//! One provider process: spawn it, keep its logs, wait until it listens.

use anyhow::{Result, bail};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::io::{BufRead, BufReader, Read};
use std::net::SocketAddr;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::direct::{PortListen, STRANGER_LISTEN, port_listen};
use super::forward::Stop;
use super::logs::{dedupe_lines, printed_addr};
use super::{Direct, Endpoint, Inner, READINESS_POLL_INTERVAL, configure_child_process};

// A persistent kubectl logs a line per connection for hours.
const MAX_LOG_BYTES: usize = 64 * 1024;

type Logs = Arc<Mutex<Vec<u8>>>;

pub(super) struct ChildForward {
    pub(super) child: Child,
    /// The child's stdin, out of `child` since `Child::wait` closes it.
    /// `lade tunnel-guard` stops the provider once it closes, however Lade
    /// exits.
    _lifeline: Option<ChildStdin>,
    logs: Logs,
    printed: Arc<Mutex<Option<SocketAddr>>>,
    stdout_join: Option<JoinHandle<()>>,
    stderr_join: Option<JoinHandle<()>>,
}

pub(super) fn spawn_forward(mut command: Command, inner: &Inner) -> Result<ChildForward> {
    configure_child_process(&mut command);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let lifeline = child.stdin.take();
    let logs: Logs = Arc::new(Mutex::new(Vec::new()));
    let printed = Arc::new(Mutex::new(None));
    let stdout_join = child.stdout.take().map(|pipe| {
        spawn_pipe_reader(
            pipe,
            Arc::clone(&logs),
            matches!(inner, Inner::Printed).then(|| Arc::clone(&printed)),
        )
    });
    let stderr_join = child
        .stderr
        .take()
        .map(|pipe| spawn_pipe_reader(pipe, Arc::clone(&logs), None));
    Ok(ChildForward {
        child,
        _lifeline: lifeline,
        logs,
        printed,
        stdout_join,
        stderr_join,
    })
}

impl ChildForward {
    pub(super) fn logs_text(&self) -> String {
        dedupe_lines(&String::from_utf8_lossy(
            &self.logs.lock().expect("logs mutex"),
        ))
    }

    pub(super) fn join_readers(&mut self) {
        if let Some(handle) = self.stdout_join.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.stderr_join.take() {
            let _ = handle.join();
        }
    }

    fn endpoint(&self, inner: &Inner) -> Option<Endpoint> {
        match inner {
            Inner::Fixed(endpoint) => Some(endpoint.clone()),
            Inner::Direct(_) => None,
            Inner::Printed => {
                let addr = (*self.printed.lock().expect("printed mutex"))?;
                Some(Endpoint::Tcp {
                    host: addr.ip().to_string(),
                    port: addr.port(),
                })
            }
        }
    }
}

pub(super) fn wait_forward_ready(
    child: &mut ChildForward,
    inner: &Inner,
    deadline: Instant,
    stop: &Stop,
) -> Result<Endpoint> {
    if let Inner::Direct(direct) = inner {
        return wait_direct_ready(child, direct, deadline, stop);
    }
    loop {
        if let Some(status) = child.child.try_wait()? {
            // Read its log to the end first: the readers may lag behind.
            stop_child(&mut child.child);
            child.join_readers();
            bail!(
                "process exited before becoming ready (status: {status}): {}",
                child.logs_text()
            );
        }
        if let Some(endpoint) = child.endpoint(inner)
            && endpoint.connect(Duration::from_millis(200)).is_ok()
        {
            return Ok(endpoint);
        }
        if Instant::now() >= deadline {
            bail!(
                "timeout waiting for the tunnel to listen: {}",
                child.logs_text()
            );
        }
        if stop.sleep(READINESS_POLL_INTERVAL) {
            bail!("tunnel stopped before readiness");
        }
    }
}

fn wait_direct_ready(
    child: &mut ChildForward,
    direct: &Direct,
    deadline: Instant,
    stop: &Stop,
) -> Result<Endpoint> {
    loop {
        if let Some(status) = child.child.try_wait()? {
            stop_child(&mut child.child);
            child.join_readers();
            bail!(
                "process exited before becoming ready (status: {status}): {}",
                child.logs_text()
            );
        }
        match port_listen(direct.port, child.child.id()) {
            PortListen::Ours => return Ok(direct.endpoint()),
            PortListen::Stranger => {
                bail!("{STRANGER_LISTEN} on {}:{}", direct.host, direct.port);
            }
            PortListen::Vacant => {}
        }
        if Instant::now() >= deadline {
            bail!(
                "timeout waiting for the tunnel to listen: {}",
                child.logs_text()
            );
        }
        if stop.sleep(READINESS_POLL_INTERVAL) {
            bail!("tunnel stopped before readiness");
        }
    }
}

/// The whole group: a member left alive (an exec credential plugin, a
/// ProxyCommand) would hold the log pipes open and hang the readers.
pub(super) fn stop_child(child: &mut Child) {
    if let Ok(pid) = i32::try_from(child.id()) {
        let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
    }
    let _ = child.wait();
}

fn push_log(logs: &Logs, line: &[u8]) {
    let mut logs = logs.lock().expect("logs mutex");
    logs.extend_from_slice(line);
    if logs.len() > MAX_LOG_BYTES {
        let excess = logs.len() - MAX_LOG_BYTES;
        logs.drain(..excess);
    }
}

fn spawn_pipe_reader<R: Read + Send + 'static>(
    reader: R,
    logs: Logs,
    printed: Option<Arc<Mutex<Option<SocketAddr>>>>,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = BufReader::new(reader);
        let mut line = Vec::new();
        while reader.read_until(b'\n', &mut line).is_ok_and(|n| n > 0) {
            if let Some(slot) = &printed
                && let Some(addr) = printed_addr(&String::from_utf8_lossy(&line))
            {
                slot.lock().expect("printed mutex").get_or_insert(addr);
            }
            push_log(&logs, &line);
            line.clear();
        }
    })
}
