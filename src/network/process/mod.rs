mod forward;
mod logs;

#[cfg(test)]
mod tests;

pub(crate) use forward::RunningForward;
pub(crate) use logs::ChildOutputFiles;

use anyhow::{Result, bail};
use nix::errno::Errno;
use nix::sys::signal::{Signal, kill, killpg};
use nix::unistd::Pid;
use std::net::{TcpStream, ToSocketAddrs};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

/// Delay between readiness checks while waiting for a forwarded port to
/// accept connections. Kept short for snappier detection; the dominant cost
/// is the provider CLI starting up, not this poll loop.
const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(30);

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
