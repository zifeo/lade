use anyhow::{Result, anyhow};
use std::path::Path;
use std::sync::mpsc::{self, RecvTimeoutError};

use crate::config::{NetworkBinding, RuleTtl};
use crate::hub::Lease;
use crate::network::Stop;
use crate::network::binding::{Acquired, HUB_POLL, acquire_binding};
use crate::network::lease::own_lease;
use crate::network::types::{AcquiredNetwork, DetachedNetworkSession, HubLease};
use crate::provider_progress::ProviderProgressSink;

/// The hub holds every tunnel under this process's lease, so a tunnel stops
/// with the command, or with its ttl once the last command is done.
pub fn start_attached_network_session(
    bindings: &[NetworkBinding],
    progress: ProviderProgressSink,
    cwd: &Path,
    cancel: &Stop,
) -> Result<AcquiredNetwork> {
    let mut network = AcquiredNetwork::empty();
    if bindings.is_empty() {
        return Ok(network);
    }
    let lease = own_lease()?;
    network._lease = Some(HubLease(lease.id.clone()));
    let session = crate::hub::session_ttl(cwd);
    for acquired in run_workers(bindings, &lease, session, progress, cancel)? {
        if let Some((key, value)) = acquired.env_entry {
            network.env.insert(key, value);
        }
    }
    Ok(network)
}

/// `lade set` holds the tunnels under its own lease until
/// [`DetachedNetworkSession::hand_to_shell`], so a Ctrl-C before that leaves
/// nothing behind.
pub fn start_detached_network_session(
    bindings: &[NetworkBinding],
    progress: ProviderProgressSink,
    cwd: &Path,
    cancel: &Stop,
) -> Result<DetachedNetworkSession> {
    let mut session = DetachedNetworkSession::empty();
    if bindings.is_empty() {
        return Ok(session);
    }
    let lease = own_lease()?;
    session.lease = Some(HubLease(lease.id.clone()));
    let ttl = crate::hub::session_ttl(cwd);
    for acquired in run_workers(bindings, &lease, ttl, progress, cancel)? {
        if let Some((key, value)) = acquired.env_entry {
            session.env.insert(key, value);
        }
        session.asks.push(acquired.ask);
    }
    Ok(session)
}

/// Stops the other workers at the first error or on `cancel`, then joins
/// them all: Lade exits right after, and a worker still waiting on the hub
/// would keep its lease open.
fn run_workers(
    bindings: &[NetworkBinding],
    lease: &Lease,
    session: Option<RuleTtl>,
    progress: ProviderProgressSink,
    cancel: &Stop,
) -> Result<Vec<Acquired>> {
    let (done, results) = mpsc::channel();
    let stops = bindings.iter().map(|_| Stop::default()).collect::<Vec<_>>();
    let workers = bindings
        .iter()
        .cloned()
        .zip(stops.iter().cloned())
        .enumerate()
        .map(|(index, (binding, stop))| {
            let lease = lease.clone();
            let session = session.clone();
            let progress = progress.clone();
            let done = done.clone();
            std::thread::spawn(move || {
                let result = acquire_binding(binding, lease, session, progress, &stop);
                let _ = done.send((index, result));
            })
        })
        .collect::<Vec<_>>();
    drop(done);
    let mut acquired = bindings.iter().map(|_| None).collect::<Vec<_>>();
    let mut failure = None;
    loop {
        match results.recv_timeout(HUB_POLL) {
            Ok((index, Ok(binding))) => acquired[index] = Some(binding),
            Ok((_, Err(error))) => {
                failure.get_or_insert(error);
                stops.iter().for_each(Stop::stop);
            }
            Err(RecvTimeoutError::Timeout) => {
                if cancel.is_stopped() {
                    failure.get_or_insert_with(|| anyhow!("interrupted"));
                    stops.iter().for_each(Stop::stop);
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    for worker in workers {
        if worker.join().is_err() {
            failure.get_or_insert_with(|| anyhow!("tunnel worker panicked"));
        }
    }
    match failure {
        Some(error) => Err(error),
        None => Ok(acquired.into_iter().flatten().collect()),
    }
}
