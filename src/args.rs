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
    /// Set environment for shell.
    Set(EvalCommand),
    /// Unset environment for shell.
    Unset(EvalCommand),
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
pub struct Args {
    #[clap(subcommand)]
    pub command: Command,

    #[command(flatten)]
    pub verbose: Verbosity,
}
