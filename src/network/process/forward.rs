use anyhow::{Result, anyhow};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::process::Command;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::child::{spawn_forward, stop_child, wait_forward_ready};
use super::direct::STRANGER_LISTEN;
use super::policy::{Policy, Step};
use super::{Endpoint, Inner};

pub(crate) type EndpointSlot = Arc<Mutex<Option<Endpoint>>>;

/// Stops a supervisor from another thread, also while its first start
/// still blocks [`RunningForward::supervise`].
#[derive(Debug, Clone, Default)]
pub(crate) struct Stop(Arc<(Mutex<bool>, Condvar)>);

impl Stop {
    pub(crate) fn stop(&self) {
        let (stopped, wake) = &*self.0;
        *stopped.lock().unwrap_or_else(PoisonError::into_inner) = true;
        wake.notify_all();
    }

    pub(crate) fn is_stopped(&self) -> bool {
        *self.0.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sleeps for `delay` or until stopped. True when stopped.
    pub(crate) fn sleep(&self, delay: Duration) -> bool {
        let (stopped, wake) = &*self.0;
        let guard = stopped.lock().unwrap_or_else(PoisonError::into_inner);
        let (guard, _) = wake
            .wait_timeout_while(guard, delay, |stopped| !*stopped)
            .unwrap_or_else(PoisonError::into_inner);
        *guard
    }
}

#[derive(Debug)]
pub(crate) struct RunningForward {
    stop: Stop,
    child_pid: Arc<AtomicI32>,
    endpoint: EndpointSlot,
    supervisor: Option<JoinHandle<()>>,
}

impl RunningForward {
    /// Blocks until the first child is ready, then keeps restarting it.
    pub(crate) fn supervise<F>(
        name: String,
        inner: Inner,
        stop: Stop,
        command: F,
    ) -> Result<(Self, u32)>
    where
        F: Fn() -> Result<Command> + Send + 'static,
    {
        let child_pid = Arc::new(AtomicI32::new(0));
        let endpoint: EndpointSlot = Arc::new(Mutex::new(None));
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let shared = Shared {
            stop: stop.clone(),
            child_pid: Arc::clone(&child_pid),
            endpoint: Arc::clone(&endpoint),
        };
        let supervisor =
            std::thread::spawn(move || supervise(&name, &inner, command, shared, ready_tx));
        let pid = ready_rx
            .recv()
            .map_err(|_| anyhow!("tunnel supervisor stopped before readiness"))??;
        Ok((
            Self {
                stop,
                child_pid,
                endpoint,
                supervisor: Some(supervisor),
            },
            pid,
        ))
    }

    /// `None` while the provider restarts.
    pub(crate) fn endpoint(&self) -> EndpointSlot {
        Arc::clone(&self.endpoint)
    }
}

impl Drop for RunningForward {
    fn drop(&mut self) {
        self.stop.stop();
        let pid = self.child_pid.load(Ordering::Acquire);
        if pid > 0 {
            let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
        }
        if let Some(handle) = self.supervisor.take() {
            let _ = handle.join();
        }
    }
}

struct Shared {
    stop: Stop,
    child_pid: Arc<AtomicI32>,
    endpoint: EndpointSlot,
}

fn set_endpoint(slot: &EndpointSlot, value: Option<Endpoint>) {
    *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = value;
}

fn supervise<F>(
    name: &str,
    inner: &Inner,
    command: F,
    shared: Shared,
    ready_tx: mpsc::SyncSender<Result<u32>>,
) where
    F: Fn() -> Result<Command>,
{
    let origin = Instant::now();
    let mut policy = Policy::new();
    loop {
        if shared.stop.is_stopped() {
            if policy.shutdown() {
                let _ = ready_tx.send(Err(anyhow!("tunnel stopped before readiness")));
            }
            return;
        }
        let run = run_once(
            name,
            inner,
            &command,
            &shared,
            &mut policy,
            origin,
            &ready_tx,
        );
        let delay = match run {
            Run::Stopped => continue,
            Run::Exited => policy.exited(),
            Run::LostPort(error) => {
                if !policy.reported() {
                    let _ = ready_tx.send(Err(error));
                }
                return;
            }
            Run::NotReady(phase, error) => match policy.not_ready(origin.elapsed()) {
                Step::Fail => {
                    let _ = ready_tx.send(Err(error));
                    return;
                }
                Step::Retry(delay) => {
                    log::warn!("{name} {phase}: {error}");
                    delay
                }
            },
        };
        // A stop during the wait is seen at the top of the loop.
        shared.stop.sleep(delay);
    }
}

/// How one provider run ended.
enum Run {
    NotReady(&'static str, anyhow::Error),
    /// It was ready, then exited on its own.
    Exited,
    Stopped,
    LostPort(anyhow::Error),
}

/// Spawns the provider and waits until it exits.
fn run_once<F>(
    name: &str,
    inner: &Inner,
    command: &F,
    shared: &Shared,
    policy: &mut Policy,
    origin: Instant,
    ready_tx: &mpsc::SyncSender<Result<u32>>,
) -> Run
where
    F: Fn() -> Result<Command>,
{
    if let Inner::Direct(direct) = inner {
        direct.yield_port();
    }
    let mut child = match command().and_then(|command| spawn_forward(command, inner)) {
        Ok(child) => child,
        Err(error) => return Run::NotReady("failed to start", error),
    };
    shared
        .child_pid
        .store(child.child.id() as i32, Ordering::Release);
    let deadline = origin + policy.listen_deadline(origin.elapsed());
    let endpoint = match wait_forward_ready(&mut child, inner, deadline, &shared.stop) {
        Ok(endpoint) => endpoint,
        Err(error) => {
            stop_child(&mut child.child);
            shared.child_pid.store(0, Ordering::Release);
            child.join_readers();
            if shared.stop.is_stopped() {
                return Run::Stopped;
            }
            if error.to_string().contains(STRANGER_LISTEN) {
                return Run::LostPort(error);
            }
            return Run::NotReady("failed before readiness", error);
        }
    };
    set_endpoint(&shared.endpoint, Some(endpoint));
    if policy.ready() {
        let _ = ready_tx.send(Ok(child.child.id()));
    }
    let status = child.child.wait();
    // Helpers it left in its group (a ProxyCommand) would hold the log
    // pipes open, and the readers would never let it restart.
    let _ = killpg(Pid::from_raw(child.child.id() as i32), Signal::SIGKILL);
    set_endpoint(&shared.endpoint, None);
    shared.child_pid.store(0, Ordering::Release);
    child.join_readers();
    if shared.stop.is_stopped() {
        return Run::Stopped;
    }
    log::warn!(
        "{name} exited after readiness (status: {}): {}",
        status.map_or_else(|error| error.to_string(), |status| status.to_string()),
        child.logs_text()
    );
    Run::Exited
}
