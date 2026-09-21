use anyhow::Result;
use clap::{Parser, Subcommand};
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::Duration;

use super::HookAgent;

/// This git repo: install locked packages, first-time pre-exec, repo
/// pre-tool. The lock is the version. `lade update` re-resolves.
/// `--unlock` ignores the lock this once.
#[derive(Parser, Debug)]
pub struct SetupCommand {
    /// Ignore the lock, resolve from yaml, rewrite, and install.
    #[clap(long, default_value_t = false)]
    pub unlock: bool,
    /// Limit pre-tool installs to these harnesses (`cursor`, `claude`,
    /// `codex`, `opencode`). Repeat for several. Skips the harness prompt.
    #[clap(long = "harness", action = clap::ArgAction::Append)]
    pub harness: Vec<HookAgent>,
}

impl SetupCommand {
    pub fn slugs(&self) -> Vec<&'static str> {
        self.harness.iter().map(|agent| agent.slug()).collect()
    }
}

/// Remove this repo's Lade pre-tool hooks and run teardown commands.
/// `--global` wipes this machine's cache. Home hooks stay.
#[derive(Parser, Debug)]
pub struct TeardownCommand {
    /// Wipe this machine's Lade cache (mise isolation, env sidecars, tickets).
    /// Home hooks stay until `lade hook disable --scope user`.
    #[clap(long, default_value_t = false)]
    pub global: bool,
}

#[derive(Parser, Debug)]
pub struct AddCommand {
    /// Family (`secret`, `package`, `tunnel`) or a package search (`ghjk`).
    #[clap(value_parser)]
    pub family: Option<String>,
    /// Search or package name after the family.
    #[clap(value_parser)]
    pub query: Option<String>,
    /// Regex rule to write. Required without a TTY.
    #[clap(long)]
    pub rule: Option<String>,
    /// Env or port name on that rule.
    #[clap(long)]
    pub key: Option<String>,
    /// URI to write (`op://`, `mise://`, `kubectl://`).
    #[clap(long)]
    pub uri: Option<String>,
}

#[derive(Parser, Debug)]
pub struct RemoveCommand {
    /// Family (`secret`, `package`, `tunnel`) or the key to drop.
    #[clap(value_parser)]
    pub family: Option<String>,
    /// Key or package name after the family.
    #[clap(value_parser)]
    pub query: Option<String>,
    /// Regex rule that holds the binding.
    #[clap(long)]
    pub rule: Option<String>,
    /// Env or port name to drop.
    #[clap(long)]
    pub key: Option<String>,
}

#[derive(Parser, Debug)]
pub struct UpgradeCommand {
    /// Install this version (e.g. 1.0.0)
    #[clap(long)]
    pub version: Option<String>,

    /// Skip the version confirm
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
#[command(hide = true)]
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
    /// Check all supported secret providers, not only those referenced in lade.yaml.
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
    /// Same as `--all`. Widens prune past this repo.
    #[clap(long, default_value_t = false, conflicts_with = "path", global = true)]
    pub global: bool,
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
    /// Check the diary hash chain.
    Verify,
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
    /// Same as `--all`.
    #[clap(long, default_value_t = false, conflicts_with = "path")]
    pub global: bool,
    /// Scope to the git root of this path. Worktrees count.
    #[clap(long, conflicts_with = "all")]
    pub path: Option<PathBuf>,
    /// Pack file, directory of packs, or `local` for the live diary.
    #[clap(long, action = clap::ArgAction::Append)]
    pub source: Vec<String>,
}
