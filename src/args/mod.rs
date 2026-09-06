use anyhow::Result;
use clap::Subcommand;
use clap_verbosity_flag::Verbosity;

use clap::{CommandFactory, Parser};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Parser, Debug)]
pub struct UpgradeCommand {
    /// Upgrade to specific version (e.g. 1.0.0)
    #[clap(long)]
    pub version: Option<String>,

    /// Do not ask for version confirmation
    #[clap(short, long, default_value_t = false)]
    pub yes: bool,
}

#[derive(Parser, Debug)]
pub struct EvalCommand {
    #[clap(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
    pub commands: Vec<String>,
}

/// Default replacement format: `{}` is replaced by the variable name.
/// Produces bash self-rehydrating tokens like `${MY_VAR:-REDACTED}`.
pub const DEFAULT_MASK_FORMAT: &str = "${{}:-REDACTED}";

#[derive(Parser, Debug)]
pub struct InjectCommand {
    /// Do not mask secret values in the subprocess output.
    #[clap(long, default_value_t = false)]
    pub no_mask: bool,
    /// Format used for masked values. `{}` is substituted with the variable
    /// name; omit `{}` for a static replacement (e.g. `REDACTED`).
    #[clap(long, default_value = DEFAULT_MASK_FORMAT)]
    pub mask_format: String,
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub commands: Vec<String>,
}

#[derive(Parser, Debug)]
pub struct McpCommand {
    /// Remote Streamable HTTP MCP endpoint.
    pub url: Option<String>,
    /// Local stdio MCP server command. It must follow `--`.
    #[arg(last = true, allow_hyphen_values = true)]
    pub argv: Vec<OsString>,
}

#[derive(Parser, Debug)]
pub struct StatusCommand {
    /// Check all supported vault CLIs, not only those referenced in lade.yml.
    #[clap(long, default_value_t = false)]
    pub all: bool,
    /// Emit a machine-readable JSON report to stdout instead of human text.
    #[clap(long, default_value_t = false)]
    pub json: bool,
}

#[derive(Parser, Debug)]
pub struct BenchCommand {
    /// Emit a machine-readable JSON report to stdout instead of human text.
    #[clap(long, default_value_t = false)]
    pub json: bool,
    /// Per-rule hydrate cap, for example `5s` or `500ms`.
    #[clap(long, default_value = "5s", value_parser = parse_timeout)]
    pub timeout: Duration,
}

pub fn parse_timeout(raw: &str) -> Result<Duration, String> {
    let raw = raw.trim();
    let (number, unit) = if let Some(number) = raw.strip_suffix("ms") {
        (number, "ms")
    } else if let Some(number) = raw.strip_suffix('s') {
        (number, "s")
    } else if let Some(number) = raw.strip_suffix('m') {
        (number, "m")
    } else {
        return Err("use a duration like 5s, 500ms, or 2m".to_string());
    };
    let amount: u64 = number
        .trim()
        .parse()
        .map_err(|_| format!("invalid duration '{raw}'"))?;
    let timeout = match unit {
        "ms" => Duration::from_millis(amount),
        "s" => Duration::from_secs(amount),
        "m" => Duration::from_secs(amount.saturating_mul(60)),
        _ => unreachable!("unit is one of ms, s, m"),
    };
    if timeout.is_zero() {
        return Err("timeout must be greater than 0".to_string());
    }
    Ok(timeout)
}

pub const DURATION_HELP: &str = "Ns | Nm | Nh | Nd | Nw | Nmonth. m is minutes. month is calendar months. Examples: 30m, 2h, 7d, 1month.";

#[derive(Parser, Debug)]
pub struct LogCommand {
    /// Start of the window. Default `90d` unless `--limit` is set alone.
    #[clap(long, help = DURATION_HELP, global = true)]
    pub since: Option<String>,
    /// End of the window, as a duration back from now.
    #[clap(long, help = DURATION_HELP, global = true)]
    pub until: Option<String>,
    /// Max rows, newest first. Extra cap, combinable with `--since` / `--until`.
    #[clap(long, global = true)]
    pub limit: Option<usize>,
    /// `all`, `human`, or `agent`.
    #[clap(long, default_value = "all", global = true)]
    pub audience: String,
    /// `all`, `seen`, `access`, or `denied`.
    #[clap(long, default_value = "all", global = true)]
    pub kind: String,
    /// `command` aggregates by stored command text, most frequent first.
    #[clap(long, global = true)]
    pub group: Option<String>,
    /// Emit stored fields as JSON.
    #[clap(long, default_value_t = false, global = true)]
    pub json: bool,
    /// Drop the git-root filter and read the whole diary.
    #[clap(long, default_value_t = false, conflicts_with = "path", global = true)]
    pub all: bool,
    /// Scope to the git root of this path. Worktrees count.
    #[clap(long, conflicts_with = "all", global = true)]
    pub path: Option<PathBuf>,
    /// Pack file, directory of packs, or `local` for the live diary.
    #[clap(long, action = clap::ArgAction::Append, global = true)]
    pub source: Vec<String>,
    #[clap(subcommand)]
    pub action: Option<LogAction>,
}

#[derive(Subcommand, Debug)]
pub enum LogAction {
    /// Delete rows older than `--keep`. Required. Nothing prunes by itself.
    Prune {
        /// Keep this window. Ns | Nm | Nh | Nd | Nw | Nmonth.
        #[clap(long, help = DURATION_HELP)]
        keep: Option<String>,
    },
    /// Write a gzipped SQLite snapshot of the diary window.
    Share {
        /// Output path. Default is `lade-$USER-$FROM-$TO.tar.gz` in cwd.
        #[clap(short, long)]
        output: Option<PathBuf>,
    },
}

#[derive(Parser, Debug)]
pub struct UsageCommand {
    /// Start of the window. Default `90d` unless `--limit` is set alone.
    #[clap(long, help = DURATION_HELP)]
    pub since: Option<String>,
    /// End of the window, as a duration back from now.
    #[clap(long, help = DURATION_HELP)]
    pub until: Option<String>,
    /// Max rule rows, most frequent first. Extra cap, combinable with `--since` / `--until`.
    #[clap(long)]
    pub limit: Option<usize>,
    /// `all`, `human`, or `agent`.
    #[clap(long, default_value = "all")]
    pub audience: String,
    /// Emit matched rules as JSON.
    #[clap(long, default_value_t = false)]
    pub json: bool,
    /// Drop the git-root filter and read the whole diary.
    #[clap(long, default_value_t = false, conflicts_with = "path")]
    pub all: bool,
    /// Scope to the git root of this path. Worktrees count.
    #[clap(long, conflicts_with = "all")]
    pub path: Option<PathBuf>,
    /// Pack file, directory of packs, or `local` for the live diary.
    #[clap(long, action = clap::ArgAction::Append)]
    pub source: Vec<String>,
}

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
    /// Install auto launcher in shell profile.
    Install,
    /// Uninstall auto launcher in shell profile.
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
    /// Handle agent preToolUse / MCP verb JSON on stdin. Called by the preTool hook.
    #[command(hide = true)]
    Hook {
        /// Host that installed this hook. Unknown values are ignored.
        #[clap(long)]
        harness: Option<String>,
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
        Some(Command::Hook { .. }) => return print_hidden_command(&mut cmd, "hook"),
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
    for name in ["set", "unset", "hook"] {
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
