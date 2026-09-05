use clap::Subcommand;
use clap_verbosity_flag::Verbosity;

use clap::Parser;
use std::ffi::OsString;
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
    /// Set environment for shell.
    Set(EvalCommand),
    /// Unset environment for shell.
    Unset(EvalCommand),
    /// Evaluate a secret URI and print its resolved value.
    Eval {
        /// The secret URI to resolve (e.g., op://vault/item/field)
        uri: String,
    },
    /// Handle preToolUse for Cursor and Claude Code.
    Hook,
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
    /// Shortcut for `lade inject <command...>`.
    #[command(external_subcommand)]
    InjectAlias(Vec<String>),
}

#[derive(Parser, Debug)]
#[clap(name="lade", about, long_about = None, disable_version_flag = true, disable_help_flag = true)]
pub struct Args {
    #[clap(long, value_parser)]
    pub version: bool,

    #[clap(short, long, value_parser)]
    pub help: bool,

    #[clap(subcommand)]
    pub command: Option<Command>,

    #[command(flatten)]
    pub verbose: Verbosity,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bench_timeout_defaults_to_five_seconds() {
        let args = Args::try_parse_from(["lade", "bench"]).unwrap();
        match args.command {
            Some(Command::Bench(bench)) => {
                assert!(!bench.json);
                assert_eq!(bench.timeout, Duration::from_secs(5));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn bench_timeout_parses_ms() {
        let args = Args::try_parse_from(["lade", "bench", "--timeout", "500ms"]).unwrap();
        match args.command {
            Some(Command::Bench(bench)) => assert_eq!(bench.timeout, Duration::from_millis(500)),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parse_timeout_rejects_zero_and_bare_number() {
        assert!(parse_timeout("0s").is_err());
        assert!(parse_timeout("5").is_err());
    }
}
