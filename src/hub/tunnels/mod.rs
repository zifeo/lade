#[cfg(test)]
mod effects_tests;
mod machine;
#[cfg(test)]
mod model_tests;

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use super::wire::{TunnelAsk, TunnelLaunch, TunnelRep, TunnelRow};
use crate::network::{
    ClaimError, Outer, Stop, Tunnel, TunnelRequest, process_started, start_tunnel,
};
use machine::{Machine, Open};

// A liveness check costs a syscall per lease, and a starting tunnel is
// polled every 50 ms.
const LEASE_CHECK: Duration = Duration::from_secs(1);

/// A start thread leaves its result here. Once the hub drops its handle,
/// the thread's handle is the last one, so a late tunnel stops on its own.
type StartSlot = Arc<Mutex<Option<Result<(Tunnel, u32), String>>>>;

struct Start {
    slot: StartSlot,
    stop: Stop,
    thread: JoinHandle<()>,
    holds: Holds,
}

/// Port, key, and URI: a start thread binds the port and keeps it until
/// it ends, cancelled or not.
type Holds = (u16, String, String);

struct Stopping {
    thread: JoinHandle<()>,
    holds: Option<Holds>,
}

/// Effects of [`Machine`]: bind ports, run start threads, own the tunnels.
/// Every decision is the machine's.
#[derive(Default)]
pub(super) struct Tunnels {
    machine: Machine,
    starting: HashMap<u64, Start>,
    /// Cancelled starts and stopping tunnels, joined before the hub exits.
    stopping: Vec<Stopping>,
    running: HashMap<u64, Tunnel>,
    last_check: Option<Duration>,
}

/// The process exits right after: a start thread still running would die
/// mid-start and leave its provider behind.
impl Drop for Tunnels {
    fn drop(&mut self) {
        for start in self.starting.values() {
            start.stop.stop();
        }
        let threads = self.starting.drain().map(|(_, start)| start.thread);
        let stopping = self.stopping.drain(..).map(|stopping| stopping.thread);
        for thread in threads.chain(stopping) {
            let _ = thread.join();
        }
    }
}

impl Tunnels {
    pub(super) fn is_empty(&self) -> bool {
        self.machine.is_empty()
    }

    pub(super) fn open(&mut self, ask: TunnelAsk) -> TunnelRep {
        self.settle_starts();
        match self.machine.open(&ask) {
            Open::Reply(rep) => rep,
            Open::Claim { launch, yielded } => {
                self.stop(yielded.into_iter().collect());
                self.claim_and_start(&ask, launch)
            }
        }
    }

    fn claim_and_start(&mut self, ask: &TunnelAsk, launch: TunnelLaunch) -> TunnelRep {
        let outer = match Outer::claim(&ask.host, ask.port) {
            Ok(outer) => outer,
            Err(ClaimError::InUse { port }) => {
                // The machine forgot a cancelled start at once, but its
                // thread frees the port only when it ends: wait for it.
                let holder = self
                    .stopping
                    .iter()
                    .filter_map(|stopping| stopping.holds.as_ref())
                    .find(|(held, ..)| *held == port)
                    .map(|(_, key, uri)| (key.clone(), uri.clone()));
                return TunnelRep::Conflict { port, holder };
            }
            Err(ClaimError::Other(e)) => {
                return TunnelRep::Failed {
                    error: format!("{e:#}"),
                };
            }
        };
        let port = outer.port;
        let id = self.machine.admit(ask, port);
        let request = request_for(&ask.key, &ask.uri, launch);
        let slot: StartSlot = Arc::new(Mutex::new(None));
        let done = Arc::clone(&slot);
        let stop = Stop::default();
        let cancel = stop.clone();
        let thread = std::thread::spawn(move || {
            let result = start_tunnel(outer, &request, cancel).map_err(|e| format!("{e:#}"));
            *done.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(result);
        });
        let holds = (port, ask.key.clone(), ask.uri.clone());
        self.starting.insert(
            id,
            Start {
                slot,
                stop,
                thread,
                holds,
            },
        );
        TunnelRep::Starting { id, port }
    }

    /// Feeds finished starts to the machine.
    fn settle_starts(&mut self) {
        self.stopping
            .retain(|stopping| !stopping.thread.is_finished());
        let finished = self
            .starting
            .iter()
            .filter_map(|(id, start)| {
                start
                    .slot
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .take()
                    .map(|result| (*id, result))
            })
            .collect::<Vec<_>>();
        for (id, result) in finished {
            self.starting.remove(&id);
            match result {
                Ok((tunnel, pid)) => {
                    if self.machine.started(id, Ok(pid)) {
                        self.running.insert(id, tunnel);
                    }
                }
                Err(error) => {
                    self.machine.started(id, Err(error));
                }
            }
        }
    }

    fn stop(&mut self, ids: Vec<u64>) {
        for id in ids {
            if let Some(start) = self.starting.remove(&id) {
                start.stop.stop();
                self.stopping.push(Stopping {
                    thread: start.thread,
                    holds: Some(start.holds),
                });
            }
            // The port is free for the next ask at once. The provider stops
            // off the request loop, which a client gives 20 ms.
            if let Some(tunnel) = self.running.remove(&id) {
                let provider = tunnel.release_port();
                self.stopping.push(Stopping {
                    thread: std::thread::spawn(move || drop(provider)),
                    holds: None,
                });
            }
        }
    }

    pub(super) fn poll(&mut self, id: u64) -> TunnelRep {
        self.settle_starts();
        self.machine.poll(id)
    }

    pub(super) fn release(&mut self, lease: &str, now: Duration) {
        self.settle_starts();
        let gone = self.machine.release(lease, now);
        self.stop(gone);
    }

    pub(super) fn tick(&mut self, now: Duration) {
        self.settle_starts();
        if self
            .last_check
            .is_none_or(|at| now.saturating_sub(at) >= LEASE_CHECK)
        {
            self.last_check = Some(now);
            self.machine
                .drop_dead_leases(|lease| process_started(lease.pid) == Some(lease.started));
        }
        let gone = self.machine.expire(now);
        self.stop(gone);
    }

    pub(super) fn forget(&mut self, names: &[String]) {
        let gone = self.machine.forget(names);
        self.stop(gone);
    }

    pub(super) fn rows(&self, now: Duration) -> Vec<TunnelRow> {
        self.machine.rows(now)
    }
}

fn request_for(key: &str, uri: &str, launch: TunnelLaunch) -> TunnelRequest {
    TunnelRequest {
        key: key.to_string(),
        uri: uri.to_string(),
        context: launch.context,
        env: Some(
            launch
                .env
                .into_iter()
                .map(|(k, v)| (OsString::from_vec(k), OsString::from_vec(v)))
                .collect(),
        ),
    }
}
