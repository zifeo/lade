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
    /// Install a newer Lade binary.
    #[command(after_help = UPGRADE_AFTER_HELP)]
    Upgrade(UpgradeCommand),
    /// Version, hooks, mise, providers, hub counts.
    Status(StatusCommand),
    /// Time config parse, match, and per-rule secret resolution.
    #[command(hide = true)]
    Bench(BenchCommand),
    /// Enable pre-exec in this shell.
    #[command(after_help = ON_AFTER_HELP)]
    On,
    /// Disable pre-exec in this shell.
    #[command(after_help = OFF_AFTER_HELP)]
    Off,
    /// This repo: packages, lock, first-time pre-exec, repo pre-tool.
    Setup(SetupCommand),
    /// Re-resolve ranged pins and rewrite the lock.
    #[command(after_help = UPDATE_AFTER_HELP)]
    Update,
    /// Remove repo pre-tool hooks. `--global` wipes on-disk cache.
    Teardown(TeardownCommand),
    /// Write a secret, package, or tunnel into the nearest lade.yaml.
    Add(AddCommand),
    /// Drop a binding from the nearest lade.yaml.
    Remove(RemoveCommand),
    /// Run a command with matching lade.yaml access. One-shot, no pre-exec.
    Inject(InjectCommand),
    /// Wrap a local or remote MCP server with matching access.
    #[command(after_help = MCP_AFTER_HELP)]
    Mcp(McpCommand),
    /// Set environment for the interactive shell. Called by pre-exec.
    #[command(hide = true)]
    Set(EvalCommand),
    /// Restore the shell environment. Called by pre-exec.
    #[command(hide = true)]
    Unset(EvalCommand),
    /// Resolve one secret URI to stdout.
    #[command(after_help = EVAL_AFTER_HELP)]
    Eval {
        /// Diary command name. age-plugin-lade passes its binary name.
        #[arg(long = "access-command", hide = true)]
        access_command: Option<String>,
        /// Secret URI (`op://`, `file://`, `vault://`, …).
        #[arg(required_unless_present = "help")]
        uri: Option<String>,
    },
    /// Install or remove a pre-tool hook, or handle hook JSON on stdin.
    #[command(hide = true)]
    Hook {
        /// Harness that installed this hook. Unknown values are ignored.
        #[clap(long = "harness")]
        harness: Option<String>,
        #[command(subcommand)]
        action: Option<HookAction>,
    },
    /// Approve a withheld disclaimer and run the command.
    #[command(after_help = APPROVE_AFTER_HELP)]
    Approve {
        /// The approval code printed in the disclaimer (e.g. `ab12c`).
        code: Option<String>,
    },
    /// Per-user yaml map key. `--reset` uses the OS user.
    #[command(after_help = USER_AFTER_HELP)]
    User {
        /// The username to set
        username: Option<String>,
        /// Drop the saved user. Per-user keys fall back to the OS user.
        #[arg(long)]
        reset: bool,
    },
    /// This binary's RAM secret names. Never values.
    Cache(CacheCommand),
    /// Local command diary.
    #[command(after_help = LOG_AFTER_HELP)]
    Log(LogCommand),
    /// Matched rules in this tree, most frequent first.
    #[command(after_help = USAGE_AFTER_HELP)]
    Usage(UsageCommand),
    /// Run a command with matching access. Same as `lade -- <command...>`.
    #[command(external_subcommand)]
    InjectAlias(Vec<String>),
    /// In-memory secret casier. Spawned by set / wrap / mcp.
    #[command(hide = true)]
    Hub,
}

#[derive(Parser, Debug)]
#[clap(name="lade", about, long_about = None, disable_version_flag = true, disable_help_flag = true)]
pub struct Args {
    #[clap(long, value_parser)]
    pub version: bool,

    /// Print help. `-v` adds protocol verbs (`set`, `unset`, `hook`).
    #[clap(short, long, value_parser, global = true)]
    pub help: bool,

    /// Mark this invocation as pre-tool.
    #[clap(long, global = true, default_value_t = false, hide = true)]
    pub pretool: bool,

    #[clap(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub verbose: Verbosity,
}

pub const ROOT_AFTER_HELP: &str = "\
Examples:
  lade setup
  tofu apply
  lade -- tofu apply
  lade status
  lade cache
  lade cache set 2h
  lade cache forget AWS_ACCESS_KEY_ID
  lade eval op://vault/item/field

Vault and file stay in RAM 5m. Rule `.` ttl: off | 30s | 1h | 24h (max).
This cwd: lade cache set 2h. Ancestors share it. unset / forget drops it.
Progress: (c) cached  (o) overridden  (u) unset.
Next: lade <command> --help. Internal commands: lade --help -v.
";

pub const ROOT_VERBOSE_AFTER_HELP: &str = "\
Examples:
  lade setup
  tofu apply
  lade -- tofu apply
  lade status
  lade cache
  lade cache set 2h
  lade cache forget AWS_ACCESS_KEY_ID
  lade eval op://vault/item/field

Vault and file stay in RAM 5m. Rule `.` ttl: off | 30s | 1h | 24h (max).
This cwd: lade cache set 2h. Ancestors share it. unset / forget drops it.
Progress: (c) cached  (o) overridden  (u) unset.

Protocol verbs (this list):
  set / unset   pre-exec inject and restore
  hook          stdin pre-tool JSON; enable --scope user|project
  inject        same as lade --
  hub           in-memory secret casier
  --pretool     mark this invocation as pre-tool
";

pub fn help_lists_internal(verbose: &Verbosity) -> bool {
    verbose.log_level_filter() > log::LevelFilter::Error
}

fn subcommand_name(command: &Command) -> Option<&'static str> {
    match command {
        Command::Upgrade(_) => Some("upgrade"),
        Command::Status(_) => Some("status"),
        Command::Bench(_) => Some("bench"),
        Command::On => Some("on"),
        Command::Off => Some("off"),
        Command::Setup(_) => Some("setup"),
        Command::Update => Some("update"),
        Command::Teardown(_) => Some("teardown"),
        Command::Add(_) => Some("add"),
        Command::Remove(_) => Some("remove"),
        Command::Inject(_) => Some("inject"),
        Command::Mcp(_) => Some("mcp"),
        Command::Set(_) => Some("set"),
        Command::Unset(_) => Some("unset"),
        Command::Eval { .. } => Some("eval"),
        Command::Hook { .. } => Some("hook"),
        Command::Approve { .. } => Some("approve"),
        Command::User { .. } => Some("user"),
        Command::Cache(_) => Some("cache"),
        Command::Log(_) => Some("log"),
        Command::Usage(_) => Some("usage"),
        Command::Hub => Some("hub"),
        Command::InjectAlias(_) => None,
    }
}

pub fn print_command_help(command: &Option<Command>, db_path: &Path, verbose: bool) -> Result<()> {
    let mut cmd = Args::command();
    if verbose {
        reveal_internal(&mut cmd);
    }
    let Some(name) = command.as_ref().and_then(subcommand_name) else {
        let tail = if verbose {
            ROOT_VERBOSE_AFTER_HELP
        } else {
            ROOT_AFTER_HELP
        };
        cmd = std::mem::take(&mut cmd).after_help(tail);
        cmd.print_long_help()?;
        return Ok(());
    };
    let Some(sub) = cmd.find_subcommand_mut(name) else {
        return Ok(());
    };
    if matches!(name, "set" | "unset" | "inject" | "hook" | "hub" | "bench") {
        *sub = std::mem::take(sub).hide(false);
    }
    if name == "log" {
        let tail = format!(
            "{LOG_AFTER_HELP}\nDatabase: {}\nDuration: {}",
            db_path.display(),
            DURATION_HELP
        );
        *sub = std::mem::take(sub).after_help(tail);
    } else if name == "usage" {
        let tail = format!("{USAGE_AFTER_HELP}\nDuration: {DURATION_HELP}");
        *sub = std::mem::take(sub).after_help(tail);
    }
    sub.print_long_help()?;
    Ok(())
}

pub(super) fn reveal_internal(cmd: &mut clap::Command) {
    for name in ["set", "unset", "inject", "hook"] {
        if let Some(sub) = cmd.find_subcommand_mut(name) {
            *sub = std::mem::take(sub).hide(false);
        }
    }
    *cmd = std::mem::take(cmd).mut_arg("pretool", |arg| arg.hide(false));
}

#[cfg(test)]
mod tests;
