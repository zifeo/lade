mod acquire;
mod approve;
mod run;
mod set;
mod unset;

#[cfg(test)]
mod tests;

pub use approve::handle_approve;
pub use run::run_inject;
pub use set::handle_set;
pub use unset::handle_unset;

use anyhow::Result;
use rustc_hash::FxHashSet;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::audience::Via;
use crate::config::{Audience, Config, NetworkBinding, PreEventWork, SecretSources};
use crate::context::InvocationContext;
use crate::event::{self, Emit, Kind};
use crate::files::sleep_or_cancel;
use crate::message_box;
use crate::ticket::{self, PreEvent, TicketSecret};

fn loader_error_box(e: &anyhow::Error) -> message_box::MessageBox {
    message_box::MessageBox::new()
        .error()
        .line("Lade could not get secrets from one loader:")
        .line("")
        .paragraph(e.to_string())
}

pub(super) type SecretBundle = (
    HashMap<String, String>,
    HashMap<PathBuf, HashMap<String, String>>,
    HashMap<String, String>,
    FxHashSet<String>,
    Vec<String>,
);

pub(super) enum Acquisition<N> {
    Ready(SecretBundle, N),
    Failed(anyhow::Error),
    FailedWithFiles(anyhow::Error, HashMap<PathBuf, HashMap<String, String>>),
    FailedWithNetwork(anyhow::Error, N),
}

pub(super) struct ProviderWork {
    disclaimers: Vec<String>,
    secrets: Vec<TicketSecret>,
    op_sa: Option<String>,
    network_bindings: Vec<NetworkBinding>,
    matches: Value,
    log: bool,
    agent: Value,
    progress: SecretSources,
}

pub(super) struct SecretHydrate<'a> {
    secrets: &'a [TicketSecret],
    op_sa: Option<&'a str>,
    progress: &'a SecretSources,
    ticket_unlink: Option<&'a str>,
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

pub(super) fn emit_seen_if_walk_log(
    config: &Config,
    ctx: &InvocationContext,
    command: &str,
    current_dir: &Path,
    saved_user: &Option<String>,
) {
    if !config.log_on_walk() {
        return;
    }
    event::emit_if(
        true,
        Emit {
            kind: Kind::Seen,
            via: ctx.via,
            audience: ctx.audience,
            actor: event::actor(saved_user),
            cwd: current_dir.to_path_buf(),
            command: command.to_string(),
            hydrated: None,
            matches: json!([]),
            hydrate_ms: None,
            agent: crate::agent_meta::merge(Value::Null),
        },
    );
}

pub(super) fn public_hydrate(
    env: &HashMap<String, String>,
    files: &HashMap<PathBuf, HashMap<String, String>>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for vars in files.values() {
        for (key, value) in vars {
            if !key.starts_with('.') {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    for (key, value) in env {
        if key.starts_with('.')
            || key == crate::shell::LADE_VIA
            || key == crate::shell::LADE_RESTORE
            || key == crate::shell::LADE_NETWORK_PIDS
            || key == crate::shell::LADE_PENDING
            || key == crate::shell::LADE_T
        {
            continue;
        }
        out.insert(key.clone(), value.clone());
    }
    out
}

pub(super) async fn handle_provider_failure(ctx: &InvocationContext, e: &anyhow::Error) {
    if e.to_string().contains("network provider") {
        let mut mb = message_box::MessageBox::new()
            .error()
            .line("Lade could not start network providers:")
            .line("")
            .paragraph(e.to_string());
        if ctx.stderr_is_terminal {
            mb = mb.line("").line(error_pause_line(ctx));
        }
        mb.print_stderr();
        if ctx.stderr_is_terminal {
            sleep_or_cancel(5).await;
        }
        return;
    }
    handle_loader_failure(ctx, e).await;
}

async fn handle_loader_failure(ctx: &InvocationContext, e: &anyhow::Error) {
    let mut mb = loader_error_box(e);
    if ctx.stderr_is_terminal {
        mb = mb.line("").line(error_pause_line(ctx));
    }
    mb.print_stderr();
    if ctx.stderr_is_terminal {
        sleep_or_cancel(5).await;
    }
}

pub(super) async fn show_loader_warnings(ctx: &InvocationContext, warnings: &[String]) {
    if warnings.is_empty() {
        return;
    }
    let mut mb = message_box::MessageBox::new()
        .warning()
        .paragraphs(warnings.iter().map(String::as_str));
    if ctx.stderr_is_terminal {
        mb = mb
            .line("")
            .line("Waiting 2 seconds so this warning is visible...");
    }
    mb.print_stderr();
    if ctx.stderr_is_terminal {
        sleep_or_cancel(2).await;
    }
}

fn error_pause_line(ctx: &InvocationContext) -> &'static str {
    if ctx.is_interactive() {
        "Waiting 5 seconds before continuing... (Ctrl-C to cancel)"
    } else {
        "Waiting 5 seconds before continuing. Press Ctrl-C twice to stop the shell command."
    }
}

pub(super) fn merge_env_with_conflicts(
    env: &mut HashMap<String, String>,
    incoming: HashMap<String, String>,
) -> Result<()> {
    for (key, value) in incoming {
        match env.get(&key) {
            Some(existing) if existing != &value => {
                anyhow::bail!(
                    "conflicting env '{}' between secret/network providers: '{}' vs '{}'",
                    key,
                    existing,
                    value
                );
            }
            Some(_) => {}
            None => {
                env.insert(key, value);
            }
        }
    }
    Ok(())
}
