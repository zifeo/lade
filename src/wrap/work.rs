use anyhow::Result;
use serde_json::Value;
use std::path::PathBuf;

use crate::audience::Via;
use crate::config::{Audience, Config, NetworkBinding, PreEventWork, SecretSources};
use crate::ticket::{self, PreEvent, TicketSecret};

pub(super) struct ProviderWork {
    pub(super) disclaimers: Vec<String>,
    pub(super) secrets: Vec<TicketSecret>,
    pub(super) op_sa: Option<String>,
    pub(super) network_bindings: Vec<NetworkBinding>,
    pub(super) matches: Value,
    pub(super) log: bool,
    pub(super) agent: Value,
    pub(super) progress: SecretSources,
}

impl ProviderWork {
    pub(super) fn needs_inject(&self) -> bool {
        !self.secrets.is_empty()
            || !self.network_bindings.is_empty()
            || !self.disclaimers.is_empty()
    }

    /// Cancelled keys still need the progress path even when nothing is
    /// left to inject.
    pub(super) fn needs_providers(&self) -> bool {
        self.needs_inject() || !self.progress.cancelled.is_empty()
    }
}

pub(super) struct SecretHydrate<'a> {
    pub(super) secrets: &'a [TicketSecret],
    pub(super) op_sa: Option<&'a str>,
    pub(super) progress: &'a SecretSources,
    pub(super) ticket_unlink: Option<&'a str>,
}

pub(super) struct TicketCleanup(Option<String>);

impl TicketCleanup {
    pub(super) fn new(id: String) -> Self {
        Self(Some(id))
    }
}

impl Drop for TicketCleanup {
    fn drop(&mut self) {
        if let Some(id) = &self.0 {
            let _ = ticket::unlink(id);
        }
    }
}

pub(super) fn ticket_ready(id: Option<&str>) -> bool {
    id.is_some_and(ticket::exists)
}

pub(super) fn pre_event_from_work(
    command: &str,
    cwd: PathBuf,
    via: Via,
    audience: Audience,
    actor: Option<String>,
    work: &ProviderWork,
) -> PreEvent {
    PreEvent {
        command: command.to_string(),
        cwd,
        via: via.child_stamp().unwrap_or("unknown").to_string(),
        audience: match audience {
            Audience::Agent => "agent",
            Audience::Human => "human",
        }
        .to_string(),
        actor,
        log: work.log,
        disclaimers: work.disclaimers.clone(),
        secrets: work.secrets.clone(),
        network: work
            .network_bindings
            .iter()
            .map(|binding| ticket::TicketNetwork {
                key: binding.key.clone(),
                uri: binding.uri.clone(),
            })
            .collect(),
        matches: work.matches.clone(),
        op_sa: work.op_sa.clone(),
        agent: work.agent.clone(),
        network_pids: Vec::new(),
        pending: false,
    }
}

fn try_ticket_work(pre: &PreEvent) -> ProviderWork {
    ProviderWork {
        disclaimers: pre.disclaimers.clone(),
        secrets: pre.secrets.clone(),
        op_sa: pre.op_sa.clone(),
        network_bindings: pre
            .network
            .iter()
            .map(|binding| NetworkBinding {
                key: binding.key.clone(),
                uri: binding.uri.clone(),
            })
            .collect(),
        matches: pre.matches.clone(),
        log: pre.log,
        agent: pre.agent.clone(),
        progress: SecretSources {
            sources: pre
                .secrets
                .iter()
                .map(|secret| (secret.key.clone(), secret.source.clone()))
                .collect(),
            ..SecretSources::default()
        },
    }
}

fn walk_work(work: PreEventWork) -> ProviderWork {
    ProviderWork {
        disclaimers: work.disclaimers,
        secrets: work.secrets,
        op_sa: work.op_sa,
        network_bindings: work.network,
        matches: work.matches,
        log: work.log,
        agent: Value::Null,
        progress: work.progress,
    }
}

pub(super) async fn resolve_provider_work(
    config: &Config,
    command: &str,
    audience: Audience,
    ticket_id: Option<&str>,
    use_ticket: bool,
    saved_user: &Option<String>,
) -> Result<Option<ProviderWork>> {
    if use_ticket
        && let Some(id) = ticket_id
        && let Ok(pre) = ticket::read(id)
    {
        return Ok(Some(try_ticket_work(&pre)));
    }

    let patterned = config.collect_for_with_pattern(command, audience);
    if patterned.is_empty() {
        return Ok(None);
    }
    Ok(Some(walk_work(Config::pre_event_work(
        &patterned, saved_user,
    )?)))
}
