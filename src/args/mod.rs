use anyhow::Result;
use clap::Subcommand;
use clap_verbosity_flag::Verbosity;

use clap::{CommandFactory, Parser};
use std::path::Path;

mod commands;
mod hook;

pub use commands::*;
pub use hook::*;

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Upgrade lade.
    Upgrade(UpgradeCommand),
    /// Report lade version, config, hooks, and CLI compatibility.
    Status(StatusCommand),
    /// Time config parse, match, and per-rule secret resolution.
    Bench(BenchCommand),
    /// Enable preexec shell hooks.
    On,
    /// Disable preexec shell hooks.
    Off,
    /// Install pre-exec (this shell) and pre-tool (agents).
    Install(InstallCommand),
    /// Remove pre-exec (this shell) and pre-tool (same plane as install).
    Uninstall,
    /// Inject environment into nested command.
    Inject(InjectCommand),
    /// Resolve secrets for a local or remote MCP server.
    Mcp(McpCommand),
    /// Set environment for the interactive shell. Called by the preexec hook.
    #[command(hide = true)]
    Set(EvalCommand),
    /// Restore the shell environment. Called by the preexec hook.
    #[command(hide = true)]
    Unset(EvalCommand),
    /// Evaluate a secret URI and print its resolved value.
    Eval {
        /// The secret URI to resolve (e.g., op://vault/item/field)
        uri: String,
    },
    /// Install or remove agent preTool hooks, or handle hook JSON on stdin.
    Hook {
        /// Host that installed this hook. Unknown values are ignored.
        #[clap(long)]
        harness: Option<String>,
        #[command(subcommand)]
        action: Option<HookAction>,
    },
    /// Approve a pending disclaimer and run the command, using the code shown in
    /// the disclaimer message.
    Approve {
        /// The approval code printed in the disclaimer (e.g. `ab12c`).
        code: Option<String>,
    },
    /// Manage user
    User {
        /// The username to set
        username: Option<String>,
        /// Reset/remove the current user. lade will fallback to the OS user for secrets
        #[arg(long)]
        reset: bool,
    },
    /// Local command diary.
    Log(LogCommand),
    /// Matched lade.yml rules in this tree, most frequent first. `--all` / `--path` change the tree.
    Usage(UsageCommand),
    /// Shortcut for `lade inject <command...>`.
    #[command(external_subcommand)]
    InjectAlias(Vec<String>),
}

#[derive(Parser, Debug)]
#[clap(name="lade", about, long_about = None, disable_version_flag = true, disable_help_flag = true)]
pub struct Args {
    #[clap(long, value_parser)]
    pub version: bool,

    #[clap(short, long, value_parser, global = true)]
    pub help: bool,

    /// Stamp this invocation as preTool. Via is stored on the ticket, not the child env.
    #[clap(long, global = true, default_value_t = false, hide = true)]
    pub pretool: bool,

    #[clap(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub verbose: Verbosity,
}

pub(super) const INTERNAL_HINT: &str = "Internal commands: lade --help -v";

pub fn help_lists_internal(verbose: &Verbosity) -> bool {
    verbose.log_level_filter() > log::LevelFilter::Error
}

pub fn print_command_help(command: &Option<Command>, db_path: &Path, verbose: bool) -> Result<()> {
    let mut cmd = Args::command();
    match command {
        Some(Command::Log(_)) => {
            if let Some(sub) = cmd.find_subcommand_mut("log") {
                let tail = format!(
                    "Database: {}\nDuration: {}",
                    db_path.display(),
                    DURATION_HELP
                );
                *sub = std::mem::take(sub).after_help(tail);
                sub.print_help()?;
                return Ok(());
            }
        }
        Some(Command::Usage(_)) => {
            if let Some(sub) = cmd.find_subcommand_mut("usage") {
                let tail = format!("Duration: {DURATION_HELP}");
                *sub = std::mem::take(sub).after_help(tail);
                sub.print_help()?;
                return Ok(());
            }
        }
        Some(Command::Set(_)) => return print_hidden_command(&mut cmd, "set"),
        Some(Command::Unset(_)) => return print_hidden_command(&mut cmd, "unset"),
        Some(Command::Hook { .. }) => {
            if let Some(sub) = cmd.find_subcommand_mut("hook") {
                sub.print_help()?;
            }
            return Ok(());
        }
        Some(Command::Install(_)) => {
            if let Some(sub) = cmd.find_subcommand_mut("install") {
                sub.print_help()?;
            }
            return Ok(());
        }
        Some(Command::Uninstall) => {
            if let Some(sub) = cmd.find_subcommand_mut("uninstall") {
                sub.print_help()?;
            }
            return Ok(());
        }
        _ => {}
    }
    if verbose {
        reveal_internal(&mut cmd);
    } else {
        cmd = std::mem::take(&mut cmd).after_help(INTERNAL_HINT);
    }
    cmd.print_help()?;
    Ok(())
}

pub(super) fn reveal_internal(cmd: &mut clap::Command) {
    for name in ["set", "unset"] {
        if let Some(sub) = cmd.find_subcommand_mut(name) {
            *sub = std::mem::take(sub).hide(false);
        }
    }
    *cmd = std::mem::take(cmd).mut_arg("pretool", |arg| arg.hide(false));
}

fn print_hidden_command(cmd: &mut clap::Command, name: &str) -> Result<()> {
    if let Some(sub) = cmd.find_subcommand_mut(name) {
        *sub = std::mem::take(sub).hide(false);
        sub.print_help()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
