use clap::Subcommand;
use clap_verbosity_flag::Verbosity;

use clap::Parser;

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
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    pub commands: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Upgrade lade.
    Upgrade(UpgradeCommand),
    /// Enable execution hooks.
    On,
    /// Disable execution hooks.
    Off,
    /// Install auto launcher in shell profile.
    Install,
    /// Uninstall auto launcher in shell profile.
    Uninstall,
    /// Inject environment into nested command.
    Inject(EvalCommand),
    /// Set environment for shell.
    Set(EvalCommand),
    /// Unset environment for shell.
    Unset(EvalCommand),
    /// Manage user
    User {
        /// The username to set
        username: Option<String>,
        /// Reset/remove the current user. lade will fallback to the OS user for secrets
        #[arg(long)]
        reset: bool,
    },
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
