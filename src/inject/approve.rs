use anyhow::Result;
use std::path::PathBuf;

use crate::args::{DEFAULT_MASK_FORMAT, InjectCommand};
use crate::config::Config;
use crate::context::InvocationContext;
use crate::message_box;
use crate::prompt;
use crate::shell::Shell;
use crate::ticket;

use super::run_inject;

pub async fn handle_approve(
    ctx: &InvocationContext,
    config: &Config,
    shell: &Shell,
    current_dir: PathBuf,
    code: Option<String>,
) -> Result<Option<i32>> {
    let code = match code {
        Some(c) => c,
        None => {
            message_box::MessageBox::new()
                .error()
                .line("Run `lade approve <code>` with the code shown in the disclaimer.")
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    };
    let Some(id) = ctx.ticket_id.as_deref().filter(|id| ticket::exists(id)) else {
        message_box::MessageBox::new()
            .error()
            .line("Nothing to approve: no disclaimer is pending.")
            .print_stderr();
        std::process::exit(crate::exit_codes::FAILURE);
    };
    let pre = match ticket::read(id) {
        Ok(pre) if pre.pending => pre,
        Ok(_) => {
            message_box::MessageBox::new()
                .error()
                .line("Nothing to approve: no disclaimer is pending.")
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
        Err(_) => {
            message_box::MessageBox::new()
                .error()
                .line("The pending disclaimer state is corrupted. Re-run the command.")
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    };
    if pre.cwd != current_dir {
        message_box::MessageBox::new()
            .error()
            .line("The pending disclaimer was for a different directory:")
            .line("")
            .paragraph(pre.cwd.display().to_string())
            .print_stderr();
        std::process::exit(crate::exit_codes::FAILURE);
    }
    if !prompt::verify_code(&pre.command, &code) {
        message_box::MessageBox::new()
            .error()
            .line("Wrong or expired approval code. Re-run the command for a fresh one.")
            .print_stderr();
        std::process::exit(crate::exit_codes::FAILURE);
    }
    let opts = InjectCommand {
        no_mask: false,
        mask_format: DEFAULT_MASK_FORMAT.to_string(),
        commands: vec![],
    };
    // The code is verified, so let resolve_disclaimers through for this command.
    unsafe {
        std::env::set_var(crate::shell::LADE_APPROVE, code);
    }
    run_inject(pre.command, opts, ctx, config, shell, &current_dir).await
}
