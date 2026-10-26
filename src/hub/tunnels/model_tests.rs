//! Model check of [`Machine`]: every event sequence up to [`DEPTH`] from the
//! empty hub, plus long random walks. After each event, every invariant
//! below must hold, and from every reached state the hub must drain to
//! empty once leases die and ttls run out.

use std::collections::{BTreeSet, HashSet, VecDeque};
use std::time::Duration;

use super::super::wire::{Lease, TunnelAsk, TunnelLaunch, TunnelRep};
use super::machine::{Machine, Open, Phase, capped};

const TTL_MS: u32 = 10;
const DEPTH: usize = 8;
const CAP_DEPTH: usize = 7;
const LEASES: usize = 2;
const KEYS: [&str; 2] = ["15432", "DB_PORT"];

fn lease(n: usize) -> Lease {
    Lease {
        id: format!("L{n}"),
        pid: n as u32,
        started: 1,
    }
}

fn lease_index(lease: &Lease) -> usize {
    lease.id[1..].parse().expect("model lease id")
}

fn ask(
    key: &str,
    port: Option<u16>,
    target: &str,
    ttl_ms: Option<u32>,
    holder: usize,
) -> TunnelAsk {
    TunnelAsk {
        key: key.to_string(),
        uri: format!("kubectl://k8s.example.com:6443/ctx/{target}/service/db/5432"),
        host: "127.0.0.1".to_string(),
        port,
        fingerprint: target.to_string(),
        ttl_ms,
        lease: lease(holder),
        launch: None,
    }
}

/// Two targets on one fixed port, the same target with a ttl from another
/// shell, and an env-var key.
fn asks() -> Vec<TunnelAsk> {
    vec![
        ask("15432", Some(15432), "prd", None, 0),
        ask("15432", Some(15432), "stg", Some(TTL_MS), 1),
        ask("15432", Some(15432), "prd", Some(TTL_MS), 1),
        ask("DB_PORT", None, "prd", Some(TTL_MS), 0),
    ]
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Event {
    /// `port_free: false`: another process holds the port.
    Open {
        ask: usize,
        launch: bool,
        port_free: bool,
    },
    Started {
        id: u64,
        ok: bool,
    },
    Poll {
        id: u64,
    },
    Release {
        lease: usize,
    },
    Die {
        lease: usize,
    },
    Advance {
        ms: u64,
    },
    Tick,
    Forget {
        key: &'static str,
    },
    /// `max_cache` changed in `config.json`.
    Cap {
        ms: Option<u32>,
    },
}

/// The machine, plus what the effects layer holds for it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct World {
    machine: Machine,
    now_ms: u64,
    starting: BTreeSet<u64>,
    running: BTreeSet<u64>,
    dead: BTreeSet<usize>,
    next_ephemeral: u16,
    /// The last `max_cache` applied, tracked apart from the machine's copy.
    cap_ms: Option<u32>,
}

impl World {
    fn new() -> Self {
        Self {
            machine: Machine::default(),
            now_ms: 0,
            starting: BTreeSet::new(),
            running: BTreeSet::new(),
            dead: BTreeSet::new(),
            next_ephemeral: 50_000,
            cap_ms: None,
        }
    }

    fn now(&self) -> Duration {
        Duration::from_millis(self.now_ms)
    }

    fn stop(&mut self, gone: Vec<u64>) {
        for id in gone {
            self.starting.remove(&id);
            self.running.remove(&id);
        }
    }

    /// Mirrors `Tunnels`. Returns the reply for an `Open` or `Poll`.
    fn apply(&mut self, event: &Event) -> Option<TunnelRep> {
        match event {
            Event::Open {
                ask,
                launch,
                port_free,
            } => {
                let mut ask = asks()[*ask].clone();
                if *launch {
                    ask.launch = Some(TunnelLaunch {
                        context: None,
                        env: Vec::new(),
                    });
                }
                Some(match self.machine.open(&ask) {
                    Open::Reply(rep) => rep,
                    Open::Claim { yielded, .. } if !port_free => {
                        self.stop(yielded.into_iter().collect());
                        TunnelRep::Conflict {
                            port: ask.port.unwrap_or(0),
                            holder: None,
                        }
                    }
                    Open::Claim { yielded, .. } => {
                        self.stop(yielded.into_iter().collect());
                        let port = ask.port.unwrap_or_else(|| {
                            self.next_ephemeral += 1;
                            self.next_ephemeral
                        });
                        let id = self.machine.admit(&ask, port);
                        self.starting.insert(id);
                        TunnelRep::Starting { id, port }
                    }
                })
            }
            Event::Started { id, ok } => {
                self.starting.remove(id);
                if *ok {
                    if self.machine.started(*id, Ok(7)) {
                        self.running.insert(*id);
                    }
                } else {
                    self.machine
                        .started(*id, Err("provider exited".to_string()));
                }
                None
            }
            Event::Poll { id } => Some(self.machine.poll(*id)),
            Event::Release { lease: n } => {
                let gone = self.machine.release(&lease(*n).id, self.now());
                self.stop(gone);
                None
            }
            Event::Die { lease } => {
                self.dead.insert(*lease);
                None
            }
            Event::Advance { ms } => {
                self.now_ms += ms;
                None
            }
            Event::Tick => {
                let dead = self.dead.clone();
                self.machine
                    .drop_dead_leases(|held| !dead.contains(&lease_index(held)));
                let gone = self.machine.expire(self.now());
                self.stop(gone);
                None
            }
            Event::Forget { key } => {
                let gone = self.machine.forget(&[key.to_string()]);
                self.stop(gone);
                None
            }
            Event::Cap { ms } => {
                // Mirrors `Tunnels::set_cap`: dead leases go first.
                self.cap_ms = *ms;
                let dead = self.dead.clone();
                self.machine
                    .drop_dead_leases(|held| !dead.contains(&lease_index(held)));
                let gone = self.machine.set_cap(*ms);
                self.stop(gone);
                None
            }
        }
    }

    fn events(&self, caps: bool) -> Vec<Event> {
        let mut events = Vec::new();
        for ask in 0..asks().len() {
            if self.dead.contains(&lease_index(&asks()[ask].lease)) {
                continue;
            }
            events.push(Event::Open {
                ask,
                launch: false,
                port_free: true,
            });
            for port_free in [true, false] {
                events.push(Event::Open {
                    ask,
                    launch: true,
                    port_free,
                });
            }
        }
        for id in &self.starting {
            for ok in [true, false] {
                events.push(Event::Started { id: *id, ok });
            }
        }
        for id in self.machine.entries().keys() {
            events.push(Event::Poll { id: *id });
        }
        for n in 0..LEASES {
            events.push(Event::Release { lease: n });
            if !self.dead.contains(&n) {
                events.push(Event::Die { lease: n });
            }
        }
        events.push(Event::Advance { ms: 5 });
        events.push(Event::Advance {
            ms: u64::from(TTL_MS),
        });
        events.push(Event::Tick);
        for key in KEYS {
            events.push(Event::Forget { key });
        }
        // The hub calls `set_cap` only when the value changes.
        for ms in [Some(0), Some(5), None] {
            if caps && ms != self.cap_ms {
                events.push(Event::Cap { ms });
            }
        }
        events
    }
}

fn check(
    before: &World,
    event: &Event,
    reply: Option<&TunnelRep>,
    after: &World,
) -> Result<(), String> {
    let entries = after.machine.entries();

    // One tunnel per local port: the prd/stg clash cannot happen.
    let mut ports = HashSet::new();
    for entry in entries.values() {
        if !ports.insert(entry.port) {
            return Err(format!("two tunnels on port {}", entry.port));
        }
    }

    // The effects layer runs exactly the tunnels the machine tracks.
    let ready = entries
        .iter()
        .filter(|(_, entry)| matches!(entry.phase, Phase::Ready { .. }))
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    let starting = entries
        .iter()
        .filter(|(_, entry)| entry.phase == Phase::Starting)
        .map(|(id, _)| *id)
        .collect::<BTreeSet<_>>();
    if after.running != ready {
        return Err(format!("running {:?} but ready {:?}", after.running, ready));
    }
    if after.starting != starting {
        return Err(format!(
            "starting {:?} but tracked {:?}",
            after.starting, starting
        ));
    }

    // No tunnel ttl ever passes `max_cache`.
    for entry in entries.values() {
        if capped(entry.ttl_ms, after.cap_ms) != entry.ttl_ms {
            return Err(format!(
                "tunnel on port {} keeps {:?} past cap {:?}",
                entry.port, entry.ttl_ms, after.cap_ms
            ));
        }
    }

    // A cap change leaves only the tunnels a command holds.
    if matches!(event, Event::Cap { .. })
        && let Some(entry) = entries.values().find(|entry| entry.leases.is_empty())
    {
        return Err(format!(
            "idle tunnel on port {} survived a cap change",
            entry.port
        ));
    }

    for entry in entries.values() {
        let ids = entry
            .leases
            .iter()
            .map(|lease| &lease.id)
            .collect::<HashSet<_>>();
        if ids.len() != entry.leases.len() {
            return Err(format!("duplicate lease on port {}", entry.port));
        }
    }

    if let Event::Open {
        ask: n,
        launch,
        port_free,
    } = event
    {
        let ask = &asks()[*n];
        match reply.expect("open replies") {
            TunnelRep::Starting { id, .. } | TunnelRep::Ready { id, .. } => {
                // Never hand out a tunnel to another target.
                let entry = entries.get(id).ok_or("reply names a missing tunnel")?;
                if entry.fingerprint != ask.fingerprint {
                    return Err(format!(
                        "{} reused a tunnel to {}",
                        ask.fingerprint, entry.fingerprint
                    ));
                }
                if ask.port.is_some_and(|port| port != entry.port) {
                    return Err("reply on another port".to_string());
                }
                if !entry.leases.iter().any(|held| held.id == ask.lease.id) {
                    return Err("reply without the asker's lease".to_string());
                }
            }
            TunnelRep::Conflict {
                port,
                holder: Some((key, uri)),
            } => {
                let holder = before
                    .machine
                    .entries()
                    .values()
                    .find(|entry| entry.port == *port)
                    .ok_or("conflict names no holder")?;
                if holder.fingerprint == ask.fingerprint || &holder.key != key || &holder.uri != uri
                {
                    return Err("conflict against the asker's own target".to_string());
                }
                if matches!(holder.phase, Phase::Failed(_)) {
                    return Err("conflict against a failed start".to_string());
                }
                if holder.leases.is_empty() && matches!(holder.phase, Phase::Ready { .. }) {
                    return Err("conflict against an idle tunnel".to_string());
                }
            }
            TunnelRep::Conflict { holder: None, .. } => {
                if *port_free {
                    return Err("conflict on a free port".to_string());
                }
            }
            TunnelRep::Absent => {
                if *launch {
                    return Err("absent despite a launch".to_string());
                }
            }
            TunnelRep::Failed { .. } => {}
        }
    }

    // Every asker of a failed start sees its error, not a vanished tunnel.
    if let Event::Poll { id } = event
        && before
            .machine
            .entries()
            .get(id)
            .is_some_and(|entry| matches!(entry.phase, Phase::Failed(_)))
        && !matches!(reply, Some(TunnelRep::Failed { .. }))
    {
        return Err(format!("poll of failed tunnel {id} replied {reply:?}"));
    }

    let released = match event {
        Event::Release { lease: n } => Some(lease(*n).id),
        _ => None,
    };
    let dead_now = matches!(event, Event::Tick | Event::Cap { .. });
    for (id, entry) in before.machine.entries() {
        let live = entry
            .leases
            .iter()
            .filter(|held| Some(&held.id) != released.as_ref())
            .filter(|held| !(dead_now && before.dead.contains(&lease_index(held))))
            .count();
        let Some(next) = entries.get(id) else {
            // Why may a tunnel go?
            let allowed = match event {
                Event::Forget { key } => entry.key == *key,
                Event::Cap { .. } => live == 0,
                Event::Open { ask: n, launch, .. } => {
                    let ask = &asks()[*n];
                    let yields = *launch
                        && entry.leases.is_empty()
                        && matches!(entry.phase, Phase::Ready { .. })
                        && ask.port == Some(entry.port)
                        && ask.fingerprint != entry.fingerprint;
                    matches!(entry.phase, Phase::Failed(_)) || yields
                }
                Event::Tick | Event::Release { .. } => {
                    live == 0
                        && match (&entry.phase, entry.ttl_ms, entry.idle_since) {
                            (Phase::Failed(_), _, _) => true,
                            (_, None, _) => true,
                            (_, Some(ttl), Some(since)) => {
                                after.now().saturating_sub(since)
                                    >= Duration::from_millis(u64::from(ttl))
                            }
                            (_, Some(_), None) => false,
                        }
                }
                _ => false,
            };
            if !allowed {
                return Err(format!("tunnel {id} stopped early on {event:?}"));
            }
            continue;
        };
        // Only a cap shrinks a ttl, and a cap never grows one: a raised cap
        // must not extend what is already there.
        let cap = matches!(event, Event::Cap { .. });
        if next.ttl_ms < entry.ttl_ms && !cap {
            return Err(format!("tunnel {id} ttl shrank"));
        }
        if next.ttl_ms > entry.ttl_ms && cap {
            return Err(format!("tunnel {id} ttl grew on a cap change"));
        }
    }

    // After a tick or a release, nothing idle outlives its ttl.
    if matches!(event, Event::Tick | Event::Release { .. }) {
        for (id, entry) in entries {
            if !entry.leases.is_empty() {
                continue;
            }
            let lingers = match (&entry.phase, entry.ttl_ms, entry.idle_since) {
                (Phase::Failed(_), _, _) | (_, None, _) | (_, Some(_), None) => true,
                (_, Some(ttl), Some(since)) => {
                    after.now().saturating_sub(since) >= Duration::from_millis(u64::from(ttl))
                }
            };
            if lingers {
                return Err(format!("tunnel {id} lingers"));
            }
        }
    }
    Ok(())
}

/// From any state: once every leasing process is gone and the ttl ran out,
/// the hub holds nothing.
fn drains(world: &World) -> Result<(), String> {
    let mut world = world.clone();
    for n in 0..LEASES {
        world.apply(&Event::Die { lease: n });
    }
    let ids = world.starting.iter().copied().collect::<Vec<_>>();
    for id in ids {
        world.apply(&Event::Started { id, ok: true });
    }
    world.apply(&Event::Tick);
    world.apply(&Event::Advance {
        ms: u64::from(TTL_MS),
    });
    world.apply(&Event::Tick);
    if world.machine.is_empty() && world.running.is_empty() && world.starting.is_empty() {
        Ok(())
    } else {
        Err(format!("hub never drains: {:?}", world.machine.entries()))
    }
}

/// Cap changes are rare and widen every state, so they get their own
/// shallower search: both searches together stay as fast as one.
#[test]
fn every_event_sequence_keeps_the_invariants() {
    explore(DEPTH, false);
}

#[test]
fn every_event_sequence_with_cap_changes_keeps_the_invariants() {
    explore(CAP_DEPTH, true);
}

fn explore(depth: usize, caps: bool) {
    let start = World::new();
    let mut seen = HashSet::from([start.clone()]);
    let mut queue = VecDeque::from([(start, Vec::<Event>::new())]);
    let mut transitions = 0usize;
    while let Some((world, path)) = queue.pop_front() {
        if path.len() == depth {
            continue;
        }
        for event in world.events(caps) {
            let mut next = world.clone();
            let reply = next.apply(&event);
            transitions += 1;
            let mut trail = path.clone();
            trail.push(event.clone());
            if let Err(why) =
                check(&world, &event, reply.as_ref(), &next).and_then(|()| drains(&next))
            {
                panic!("{why}\nafter {trail:#?}");
            }
            if seen.insert(next.clone()) {
                queue.push_back((next, trail));
            }
        }
    }
    assert!(seen.len() > 10_000, "explored only {} states", seen.len());
    assert!(transitions > seen.len());
}

#[test]
fn long_random_walks_keep_the_invariants() {
    // Deterministic LCG: a failure replays.
    let mut seed: u64 = 0x5eed;
    let mut next = |bound: usize| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) as usize % bound
    };
    for _ in 0..300 {
        let mut world = World::new();
        let mut trail = Vec::new();
        for _ in 0..80 {
            let events = world.events(true);
            let event = events[next(events.len())].clone();
            let before = world.clone();
            let reply = world.apply(&event);
            trail.push(event.clone());
            if let Err(why) =
                check(&before, &event, reply.as_ref(), &world).and_then(|()| drains(&world))
            {
                panic!("{why}\nafter {trail:#?}");
            }
        }
    }
}

#[test]
fn prd_then_stg_on_one_port_is_refused() {
    let mut world = World::new();
    let prd = world.apply(&Event::Open {
        ask: 0,
        launch: true,
        port_free: true,
    });
    assert!(matches!(prd, Some(TunnelRep::Starting { .. })));
    let stg = world.apply(&Event::Open {
        ask: 1,
        launch: true,
        port_free: true,
    });
    let Some(TunnelRep::Conflict {
        port: 15432,
        holder: Some((key, uri)),
    }) = stg
    else {
        panic!("{stg:?}");
    };
    assert_eq!(key, "15432");
    assert!(uri.contains("/prd/"));
}

#[test]
fn a_ttl_keeps_the_tunnel_for_the_next_command_only_until_it_runs_out() {
    let mut world = World::new();
    world.apply(&Event::Open {
        ask: 2,
        launch: true,
        port_free: true,
    });
    world.apply(&Event::Started { id: 0, ok: true });
    world.apply(&Event::Release { lease: 1 });
    assert_eq!(world.running.len(), 1);
    let again = world.apply(&Event::Open {
        ask: 0,
        launch: false,
        port_free: true,
    });
    assert!(matches!(again, Some(TunnelRep::Ready { reused: true, .. })));
    world.apply(&Event::Release { lease: 0 });
    world.apply(&Event::Advance {
        ms: u64::from(TTL_MS) - 1,
    });
    world.apply(&Event::Tick);
    assert_eq!(world.running.len(), 1);
    world.apply(&Event::Advance { ms: 1 });
    world.apply(&Event::Tick);
    assert!(world.running.is_empty());
}

#[test]
fn a_cap_change_stops_idle_tunnels_and_caps_held_ones() {
    let mut world = World::new();
    world.apply(&Event::Open {
        ask: 2,
        launch: true,
        port_free: true,
    });
    world.apply(&Event::Started { id: 0, ok: true });
    world.apply(&Event::Open {
        ask: 3,
        launch: true,
        port_free: true,
    });
    world.apply(&Event::Started { id: 1, ok: true });
    world.apply(&Event::Release { lease: 1 });
    assert_eq!(world.running.len(), 2);
    // Raising the cap still invalidates: the idle tunnel stops.
    world.apply(&Event::Cap {
        ms: Some(TTL_MS * 10),
    });
    assert_eq!(world.running.len(), 1);
    world.apply(&Event::Cap { ms: Some(0) });
    assert_eq!(world.running.len(), 1);
    world.apply(&Event::Release { lease: 0 });
    assert!(world.machine.is_empty());
}

#[test]
fn without_a_ttl_the_tunnel_stops_with_its_last_lease() {
    let mut world = World::new();
    world.apply(&Event::Open {
        ask: 0,
        launch: true,
        port_free: true,
    });
    world.apply(&Event::Started { id: 0, ok: true });
    world.apply(&Event::Release { lease: 0 });
    assert!(world.running.is_empty());
    assert!(world.machine.is_empty());
}

#[test]
fn a_dead_shell_releases_its_lease() {
    let mut world = World::new();
    world.apply(&Event::Open {
        ask: 0,
        launch: true,
        port_free: true,
    });
    world.apply(&Event::Started { id: 0, ok: true });
    world.apply(&Event::Die { lease: 0 });
    world.apply(&Event::Tick);
    assert!(world.machine.is_empty());
}
