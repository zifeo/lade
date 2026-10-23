use anyhow::Result;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::Config;
use crate::context::InvocationContext;
use crate::files::{remove_files, split_env_files};
use crate::message_box;
use crate::preexec::Shell;
use crate::ticket;

async fn unset_output_files(
    config: &Config,
    ctx: &InvocationContext,
    command: &str,
) -> Result<HashMap<PathBuf, ()>> {
    if let Some(id) = ctx.ticket_id.as_deref()
        && let Ok(pre) = ticket::read(id)
    {
        let mut files = HashMap::new();
        for secret in pre.secrets {
            if let Some(path) = secret.output {
                files.insert(path, ());
            }
        }
        return Ok(files);
    }
    let rules = config.collect_for(command, ctx.audience);
    if rules.is_empty() {
        return Ok(HashMap::new());
    }
    let saved_user = crate::config::saved_user().await?;
    let keys = Config::keys_from_rules(&rules, &saved_user);
    let (_, files) = split_env_files(keys);
    Ok(files.into_keys().map(|path| (path, ())).collect())
}

/// Each provider led its own session, so its pid is its group.
fn stop_legacy_providers(id: &str) {
    for pid in ticket::legacy_network_pids(id) {
        if let Ok(pid) = i32::try_from(pid) {
            let _ = killpg(Pid::from_raw(pid), Signal::SIGTERM);
        }
    }
}

pub async fn handle_unset(
    ctx: &InvocationContext,
    shell: &Shell,
    config: &Config,
    commands: Vec<String>,
) -> Result<()> {
    let command = commands.join(" ");
    let files = unset_output_files(config, ctx, &command).await?;
    if let Some(id) = ctx.ticket_id.as_deref().filter(|id| ticket::is_id(id)) {
        stop_legacy_providers(id);
        crate::hub::tunnel_release(id);
        crate::hub::unlink_t(id);
        let _ = ticket::unlink(id);
    }
    remove_files(&mut files.keys())?;
    if let Ok(path) = std::env::var(crate::mise::LADE_MISE_CONFIG) {
        crate::mise::unlink_config(std::path::Path::new(&path));
    }
    let restore = match std::env::var(crate::preexec::LADE_RESTORE) {
        Err(_) => None,
        Ok(raw) => match crate::preexec::RestorePayload::decode(&raw) {
            Ok(payload) => Some(payload),
            Err(_) => {
                message_box::MessageBox::new()
                    .error()
                    .line("The previous environment snapshot is corrupted. Re-run the command.")
                    .print_stderr();
                std::process::exit(crate::exit_codes::FAILURE);
            }
        },
    };
    let env_line = restore
        .map(|payload| shell.restore(payload.env))
        .unwrap_or_default();
    let unset_keys = vec![
        crate::preexec::LADE_RESTORE.to_string(),
        crate::preexec::LADE_T.to_string(),
        crate::mise::LADE_MISE_CONFIG.to_string(),
    ];
    let meta = shell.unset(unset_keys);
    let line = [env_line, meta]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(";");
    println!("{line}");
    Ok(())
}
