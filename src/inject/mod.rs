mod acquire;
mod approve;
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
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::context::InvocationContext;
use crate::event::{self, Emit, Kind};
use crate::files::sleep_or_cancel;
use crate::message_box;

fn loader_error_box(e: &anyhow::Error) -> message_box::MessageBox {
    message_box::MessageBox::new()
        .error()
        .line("Could not load secrets from one provider:")
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

pub(crate) fn emit_seen_if_walk_log(
    config: &Config,
    ctx: &InvocationContext,
    command: &str,
    current_dir: &Path,
    saved_user: &Option<String>,
    argv: Option<serde_json::Value>,
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
            || key == crate::shell::LADE_VIA
            || key == crate::shell::LADE_RESTORE
            || key == crate::shell::LADE_T
            || key == crate::mise::LADE_MISE_CONFIG
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
            .line("Could not start network providers:")
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
