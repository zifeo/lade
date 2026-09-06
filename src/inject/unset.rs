use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::Config;
use crate::context::InvocationContext;
use crate::files::{remove_files, split_env_files};
use crate::message_box;
use crate::network;
use crate::shell::Shell;
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

pub async fn handle_unset(
    ctx: &InvocationContext,
    shell: &Shell,
    config: &Config,
    commands: Vec<String>,
) -> Result<()> {
    let command = commands.join(" ");
    let files = unset_output_files(config, ctx, &command).await?;
    if let Some(id) = ctx.ticket_id.as_deref().filter(|id| ticket::is_id(id)) {
        if let Ok(pre) = ticket::read(id) {
            network::stop_network_pids_list(&pre.network_pids);
        }
        let _ = ticket::unlink(id);
    }
    remove_files(&mut files.keys())?;
    let restore = match std::env::var(crate::shell::LADE_RESTORE) {
        Err(_) => None,
        Ok(raw) => match crate::shell::RestorePayload::decode(&raw) {
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
        crate::shell::LADE_RESTORE.to_string(),
        crate::shell::LADE_T.to_string(),
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
