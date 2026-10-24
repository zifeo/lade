mod acquire;
mod approve;
pub(crate) mod exec;
mod keys;
mod pins;
mod run;
mod set;
mod unset;
mod work;

#[cfg(test)]
mod tests;

pub use approve::handle_approve;
pub use run::run_inject;
pub use set::handle_set;
pub use unset::handle_unset;

use anyhow::Result;
use rustc_hash::FxHashSet;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::context::InvocationContext;
use crate::event::{self, Emit, Kind};
use crate::message_box;
use crate::preexec::Shell;
use keys::Keys;

fn loader_error_box(e: &anyhow::Error) -> message_box::MessageBox {
    message_box::MessageBox::new()
        .error()
        .line("Could not load secrets from one provider:")
        .line("")
        .paragraph(e.to_string())
}

pub(super) fn refuse_preexec(shell: &Shell, code: i32) -> ! {
    println!("{}", shell.block_command());
    std::process::exit(code);
}

pub(super) type SecretBundle = (
    HashMap<String, String>,
    HashMap<PathBuf, HashMap<String, String>>,
    HashMap<String, String>,
    FxHashSet<String>,
    Vec<String>,
    HashSet<String>,
);

pub(super) enum Acquisition<N> {
    Ready(SecretBundle, N),
    Failed(anyhow::Error),
    FailedWithFiles(anyhow::Error, HashMap<PathBuf, HashMap<String, String>>),
    FailedWithNetwork(anyhow::Error, N),
}

pub(crate) fn emit_seen_if_walk_log(
    config: &Config,
    ctx: &InvocationContext,
    command: &str,
    current_dir: &Path,
    saved_user: &Option<String>,
    argv: Option<serde_json::Value>,
) {
    if !config.log_on_walk_for(ctx.audience) {
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
            argv,
            hydrated: None,
            matches: json!([]),
            hydrate_ms: None,
            agent: crate::agent_meta::merge(Value::Null),
        },
    );
}

pub(crate) fn public_hydrate(
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
            || key == crate::preexec::LADE_VIA
            || key == crate::preexec::LADE_RESTORE
            || key == crate::preexec::LADE_T
            || key == crate::mise::LADE_MISE_CONFIG
        {
            continue;
        }
        out.insert(key.clone(), value.clone());
    }
    out
}

/// What a failed acquisition leads to.
pub(super) enum FailureChoice {
    RunAnyway,
    Cancel,
    Interrupted,
}

const RUN_ANYWAY_WINDOW: std::time::Duration = std::time::Duration::from_secs(5);

/// Shows the failure. A human at a terminal then has 5 s to press Enter and
/// run the command without Lade. No answer, Ctrl-C, or no human cancels it.
/// The answer comes from `/dev/tty`, never stdin: that belongs to the
/// command. `keys`: the terminal, if acquisition already read it.
async fn handle_provider_failure(
    ctx: &InvocationContext,
    e: &anyhow::Error,
    keys: Option<Keys>,
) -> FailureChoice {
    let mb = if e.to_string().contains("tunnel") || e.to_string().contains("network provider") {
        message_box::MessageBox::new()
            .error()
            .line("Could not start tunnels:")
            .line("")
            .paragraph(e.to_string())
    } else {
        loader_error_box(e)
    };
    let Some(mut keys) = keys.or_else(|| Keys::enter(ctx)) else {
        mb.print_stderr();
        return FailureChoice::Cancel;
    };
    keys.forget_typed();
    mb.line("")
        .line("Press Enter within 5 seconds to run the command anyway, without Lade. It is cancelled otherwise.")
        .print_stderr();
    tokio::select! {
        choice = tokio::time::timeout(RUN_ANYWAY_WINDOW, keys.answer()) => {
            choice.unwrap_or(FailureChoice::Cancel)
        }
        () = acquire::interrupted() => FailureChoice::Interrupted,
    }
}

/// True when Ctrl-C, `kill`, or hangup cut the pause short: the command is
/// cancelled.
pub(super) async fn show_loader_warnings(ctx: &InvocationContext, warnings: &[String]) -> bool {
    if warnings.is_empty() {
        return false;
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
    if !ctx.stderr_is_terminal {
        return false;
    }
    let mut keys = Keys::enter(ctx);
    tokio::select! {
        () = tokio::time::sleep(std::time::Duration::from_secs(2)) => false,
        () = acquire::interrupted() => true,
        () = keys::ctrl_c(&mut keys) => true,
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
                    "conflicting env '{}' between secret/tunnel providers: '{}' vs '{}'",
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
