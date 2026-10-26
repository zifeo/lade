//! The hub's tunnel lifecycle as a pure state machine: no IO, no clock, no
//! threads. [`super::Tunnels`] turns its decisions into effects, and
//! `model_tests` checks its invariants over every event sequence.

use std::collections::BTreeMap;
use std::time::Duration;

pub(super) use super::super::cap::capped;
use super::super::wire::{Lease, TunnelAsk, TunnelLaunch, TunnelPhase, TunnelRep, TunnelRow};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum Phase {
    Starting,
    Ready { pid: u32 },
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct Entry {
    pub(super) key: String,
    pub(super) uri: String,
    pub(super) port: u16,
    pub(super) fixed: bool,
    pub(super) fingerprint: String,
    pub(super) ttl_ms: Option<u32>,
    pub(super) leases: Vec<Lease>,
    pub(super) idle_since: Option<Duration>,
    pub(super) phase: Phase,
}

impl Entry {
    fn rep(&self, id: u64, reused: bool) -> TunnelRep {
        match &self.phase {
            Phase::Starting => TunnelRep::Starting {
                id,
                port: self.port,
            },
            Phase::Ready { pid } => TunnelRep::Ready {
                id,
                port: self.port,
                pid: *pid,
                reused,
            },
            Phase::Failed(error) => TunnelRep::Failed {
                error: error.clone(),
            },
        }
    }

    fn expired(&self, now: Duration) -> bool {
        if !self.leases.is_empty() {
            return false;
        }
        if matches!(self.phase, Phase::Failed(_)) {
            return true;
        }
        let Some(since) = self.idle_since else {
            return false;
        };
        match self.ttl_ms {
            None => true,
            Some(ttl) => now.saturating_sub(since) >= Duration::from_millis(u64::from(ttl)),
        }
    }
}

/// What [`Machine::open`] wants next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Open {
    Reply(TunnelRep),
    /// No tunnel to reuse: stop `yielded` if set, bind the port, start
    /// `launch`, then [`Machine::admit`].
    Claim {
        launch: TunnelLaunch,
        yielded: Option<u64>,
    },
}

/// Tunnels by id. Ids are never reused, so an effect keyed by id can never
/// reach a later tunnel.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub(super) struct Machine {
    entries: BTreeMap<u64, Entry>,
    next_id: u64,
    /// `max_cache`: `None` caps nothing, `Some(0)` keeps no tunnel past its
    /// last lease.
    cap_ms: Option<u32>,
}

impl Machine {
    pub(super) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[cfg(test)]
    pub(super) fn entries(&self) -> &BTreeMap<u64, Entry> {
        &self.entries
    }

    /// Reuse a tunnel to the same target, refuse a port that reaches
    /// another one, or ask the caller to claim the port.
    pub(super) fn open(&mut self, ask: &TunnelAsk) -> Open {
        let mut yielded = None;
        if let Some(id) = self.find(ask) {
            let entry = self.entries.get_mut(&id).expect("found entry");
            if matches!(entry.phase, Phase::Failed(_)) {
                // A failed start holds no port; a new ask starts over.
                self.entries.remove(&id);
            } else if entry.fingerprint != ask.fingerprint {
                // A ready tunnel no command uses gives way to one a command
                // needs now, instead of making it wait out the ttl.
                let idle = entry.leases.is_empty() && matches!(entry.phase, Phase::Ready { .. });
                if !idle {
                    return Open::Reply(TunnelRep::Conflict {
                        port: entry.port,
                        holder: Some((entry.key.clone(), entry.uri.clone())),
                    });
                }
                if ask.launch.is_some() {
                    self.entries.remove(&id);
                    yielded = Some(id);
                }
            } else {
                if !entry.leases.iter().any(|lease| lease.id == ask.lease.id) {
                    entry.leases.push(ask.lease.clone());
                }
                entry.idle_since = None;
                entry.ttl_ms = capped(entry.ttl_ms.max(ask.ttl_ms), self.cap_ms);
                return Open::Reply(entry.rep(id, true));
            }
        }
        match &ask.launch {
            Some(launch) => Open::Claim {
                launch: launch.clone(),
                yielded,
            },
            None => Open::Reply(TunnelRep::Absent),
        }
    }

    fn find(&self, ask: &TunnelAsk) -> Option<u64> {
        self.entries
            .iter()
            .find(|(_, entry)| match ask.port {
                Some(port) => entry.port == port,
                None => {
                    !entry.fixed && entry.key == ask.key && entry.fingerprint == ask.fingerprint
                }
            })
            .map(|(id, _)| *id)
    }

    /// The caller bound `port` for `ask`, after [`Open::Claim`].
    pub(super) fn admit(&mut self, ask: &TunnelAsk, port: u16) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.entries.insert(
            id,
            Entry {
                key: ask.key.clone(),
                uri: ask.uri.clone(),
                port,
                fixed: ask.port.is_some(),
                fingerprint: ask.fingerprint.clone(),
                ttl_ms: capped(ask.ttl_ms, self.cap_ms),
                leases: vec![ask.lease.clone()],
                idle_since: None,
                phase: Phase::Starting,
            },
        );
        id
    }

    /// A start finished. `false`: the tunnel was stopped meanwhile, so the
    /// caller drops what it started.
    pub(super) fn started(&mut self, id: u64, outcome: Result<u32, String>) -> bool {
        let Some(entry) = self.entries.get_mut(&id) else {
            return false;
        };
        if entry.phase != Phase::Starting {
            return false;
        }
        match outcome {
            Ok(pid) => {
                entry.phase = Phase::Ready { pid };
                true
            }
            Err(error) => {
                entry.phase = Phase::Failed(error);
                false
            }
        }
    }

    /// A failure stays until its leases go, so every asker sees the error.
    pub(super) fn poll(&self, id: u64) -> TunnelRep {
        self.entries
            .get(&id)
            .map_or(TunnelRep::Absent, |entry| entry.rep(id, false))
    }

    /// Returns the tunnels to stop.
    pub(super) fn release(&mut self, lease: &str, now: Duration) -> Vec<u64> {
        for entry in self.entries.values_mut() {
            entry.leases.retain(|held| held.id != lease);
        }
        self.expire(now)
    }

    pub(super) fn drop_dead_leases(&mut self, alive: impl Fn(&Lease) -> bool) {
        for entry in self.entries.values_mut() {
            entry.leases.retain(&alive);
        }
    }

    /// Starts the idle clock of unleased tunnels and returns the tunnels to
    /// stop: failed ones, and idle ones past their ttl, at once without one.
    pub(super) fn expire(&mut self, now: Duration) -> Vec<u64> {
        for entry in self.entries.values_mut() {
            if entry.leases.is_empty() {
                entry.idle_since.get_or_insert(now);
            } else {
                entry.idle_since = None;
            }
        }
        let gone = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.expired(now))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in &gone {
            self.entries.remove(id);
        }
        gone
    }

    /// `lade ttl forget KEY`. Returns the tunnels to stop.
    pub(super) fn forget(&mut self, names: &[String]) -> Vec<u64> {
        let gone = self
            .entries
            .iter()
            .filter(|(_, entry)| names.contains(&entry.key))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in &gone {
            self.entries.remove(id);
        }
        gone
    }

    /// `max_cache` changed: every tunnel no command holds stops now, and the
    /// rest keep at most `cap_ms` once idle. Returns the tunnels to stop.
    pub(super) fn set_cap(&mut self, cap_ms: Option<u32>) -> Vec<u64> {
        self.cap_ms = cap_ms;
        for entry in self.entries.values_mut() {
            entry.ttl_ms = capped(entry.ttl_ms, cap_ms);
        }
        let gone = self
            .entries
            .iter()
            .filter(|(_, entry)| entry.leases.is_empty())
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        for id in &gone {
            self.entries.remove(id);
        }
        gone
    }

    pub(super) fn rows(&self, now: Duration) -> Vec<TunnelRow> {
        let mut rows = self
            .entries
            .values()
            .map(|entry| TunnelRow {
                key: entry.key.clone(),
                uri: entry.uri.clone(),
                port: entry.port,
                leases: entry.leases.len() as u32,
                ttl_left_ms: match (entry.idle_since, entry.ttl_ms) {
                    (Some(since), Some(ttl)) => Some(ttl.saturating_sub(
                        u32::try_from(now.saturating_sub(since).as_millis()).unwrap_or(u32::MAX),
                    )),
                    _ => None,
                },
                phase: match entry.phase {
                    Phase::Starting => TunnelPhase::Starting,
                    Phase::Ready { .. } => TunnelPhase::Ready,
                    Phase::Failed(_) => TunnelPhase::Failed,
                },
            })
            .collect::<Vec<_>>();
        rows.sort_by_key(|row| row.port);
        rows
    }
}
