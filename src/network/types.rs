use anyhow::{Result, bail};
use std::collections::HashMap;

use super::lease::shell_lease;
use crate::hub::{TunnelAsk, TunnelRep};

#[derive(Debug, Clone)]
pub(crate) enum LocalTarget {
    FixedPort(u16),
    EnvVar(String),
}

pub(crate) type ProviderSpec = lade_sdk::network::ProviderSpec;

#[derive(Debug, Clone)]
pub(crate) struct ParsedBinding {
    pub(crate) target: LocalTarget,
    pub(crate) local_host: String,
    pub(crate) local_port: Option<u16>,
    pub(crate) spec: ProviderSpec,
}

/// Tunnels for one attached command, held by the hub under this process's
/// lease.
#[derive(Debug)]
pub struct AcquiredNetwork {
    pub env: HashMap<String, String>,
    pub(crate) _lease: Option<HubLease>,
}

impl AcquiredNetwork {
    pub fn empty() -> Self {
        Self {
            env: HashMap::new(),
            _lease: None,
        }
    }
}

/// Pre-exec tunnels, held in the hub under `lade set`'s own lease until
/// [`Self::hand_to_shell`].
#[derive(Debug)]
pub struct DetachedNetworkSession {
    pub env: HashMap<String, String>,
    pub(crate) lease: Option<HubLease>,
    pub(crate) asks: Vec<TunnelAsk>,
}

impl DetachedNetworkSession {
    pub fn empty() -> Self {
        Self {
            env: HashMap::new(),
            lease: None,
            asks: Vec::new(),
        }
    }

    /// The shell's lease takes over right before `lade set` prints, and
    /// `lade unset` releases it. Dropping `self` then releases this
    /// process's lease.
    pub fn hand_to_shell(self, ticket: &str) -> Result<()> {
        if self.asks.is_empty() {
            return Ok(());
        }
        let shell = shell_lease(ticket)?;
        for ask in &self.asks {
            let ask = TunnelAsk {
                lease: shell.clone(),
                ..ask.clone()
            };
            let key = ask.key.clone();
            if !handover_ready(ask) {
                crate::hub::tunnel_release(ticket);
                bail!("the Lade hub lost the tunnel for {key} before the shell took it over");
            }
        }
        Ok(())
    }
}

/// Releases a hub lease on drop.
#[derive(Debug)]
pub(crate) struct HubLease(pub(crate) String);

impl Drop for HubLease {
    fn drop(&mut self) {
        crate::hub::tunnel_release(&self.0);
    }
}

fn handover_ready(ask: TunnelAsk) -> bool {
    for _ in 0..3 {
        match crate::hub::tunnel_open(ask.clone()) {
            Some(TunnelRep::Ready { .. }) => return true,
            Some(_) => return false,
            None => {}
        }
    }
    false
}
