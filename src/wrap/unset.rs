use anyhow::Result;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::context::InvocationContext;
use crate::files::remove_files;
use crate::message_box;
use crate::preexec::Shell;
use crate::ticket;

/// The ticket of this shell's line: the one its `lade set` wrote, or a
/// pending one, its withheld line. A ticket of another command came down
/// from an outer shell, whose command still runs and still needs it.
fn own_ticket<'a>(ctx: &'a InvocationContext, command: &str) -> Option<&'a str> {
    // § gy20cgb72.e@m2bax
    ctx.ticket_id
        .as_deref()
        .filter(|id| ticket::is_id(id))
        .filter(|id| ticket::read(id).map_or(true, |pre| pre.pending || pre.command == command))
}

/// The `.file` outputs the ticket records, which Lade wrote for this line.
fn ticket_output_files(id: Option<&str>) -> HashMap<PathBuf, ()> {
    // § fz8r6d2ka.e@9sqxc
    let Some(pre) = id.and_then(|id| ticket::read(id).ok()) else {
        return HashMap::new();
    };
    pre.secrets
        .into_iter()
        .filter_map(|secret| secret.output)
        .map(|path| (path, ()))
        .collect()
}

/// Each provider led its own session, so its pid is its group.
fn stop_legacy_providers(id: &str) {
    for pid in ticket::legacy_network_pids(id) {
        if let Ok(pid) = i32::try_from(pid) {
            let _ = killpg(Pid::from_raw(pid), Signal::SIGTERM);
        }
    }
}

fn protocol_keys() -> Vec<String> {
    vec![
        crate::preexec::LADE_RESTORE.to_string(),
        crate::preexec::LADE_T.to_string(),
        crate::mise::LADE_MISE_CONFIG.to_string(),
    ]
}

/// The variables the ticket names: its secrets and its tunnels. A numeric
/// tunnel key is a local port, not a variable.
fn ticket_keys(id: Option<&str>) -> Vec<String> {
    let Some(id) = id else {
        return Vec::new();
    };
    ticket::read(id)
        .map(|pre| {
            let tunnels = pre
                .network
                .into_iter()
                .map(|binding| binding.key)
                .filter(|key| !key.bytes().all(|byte| byte.is_ascii_digit()));
            pre.secrets
                .into_iter()
                .map(|secret| secret.key)
                .chain(tunnels)
                .collect()
        })
        .unwrap_or_default()
}

pub fn handle_unset(ctx: &InvocationContext, shell: &Shell, commands: Vec<String>) -> Result<()> {
    let command = commands.join(" ");
    let id = own_ticket(ctx, &command);
    let files = ticket_output_files(id);
    // § jfxr4rmae.b@n2qga
    // A nested shell inherits the outer line's snapshot: when its own
    // `lade set` failed before it printed, that snapshot is still there.
    let restore = match std::env::var(crate::preexec::LADE_RESTORE) {
        Err(_) => None,
        Ok(raw) => match crate::preexec::RestorePayload::decode(&raw) {
            Ok(payload) if id == Some(payload.ticket.as_str()) => Some(Ok(payload)),
            Ok(_) => None,
            Err(_) => Some(Err(())),
        },
    };
    // § end jfxr4rmae.b
    // § qmybbcmkx.b@3q82j
    let smashed = matches!(restore, Some(Err(())));
    if smashed {
        message_box::MessageBox::new()
            .error()
            .line("The previous environment snapshot is corrupted. Re-run the command.")
            .print_stderr();
    }
    let smash_keys = smashed.then(|| {
        let mut keys = ticket_keys(id);
        keys.extend(protocol_keys());
        keys
    });
    // § end qmybbcmkx.b
    if let Some(id) = id {
        // § gy20cgb72.b@t9tdw
        stop_legacy_providers(id);
        crate::hub::tunnel_release(id);
        // § one f18xcggkd.b@8tdsb
        // silcrow:follow
        crate::hub::unlink_t(id);
        let _ = ticket::unlink(id);
    }
    // § one fz8r6d2ka.d@5732z
    // The restore line still goes out: the shell must lose the values even
    // when a file stays.
    let removed = remove_files(&mut files.keys());
    // § jfxr4rmae.b@pbej7
    // An inherited config belongs to the outer line, whose pins still run.
    let own_mise_config = match &restore {
        Some(Ok(payload)) => payload.env.contains_key(crate::mise::LADE_MISE_CONFIG),
        Some(Err(())) => id.is_some(),
        None => false,
    };
    if own_mise_config && let Ok(path) = std::env::var(crate::mise::LADE_MISE_CONFIG) {
        crate::mise::unlink_config(std::path::Path::new(&path));
    }
    let env_line = match (restore, smash_keys) {
        (Some(Ok(payload)), _) => shell.restore(payload.env),
        (_, Some(keys)) => shell.unset(keys),
        _ => String::new(),
    };
    // § end jfxr4rmae.b
    let meta = if smashed {
        String::new()
    } else {
        shell.unset(protocol_keys())
    };
    let line = [env_line, meta]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(";");
    println!("{line}");
    removed?;
    // § one qmybbcmkx.b@kg8vb
    if smashed {
        std::process::exit(crate::exit_codes::FAILURE);
    }
    Ok(())
}
