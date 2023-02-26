use anyhow::Result;
use clap::Subcommand;
use clap_verbosity_flag::Verbosity;
use self_update::cargo_crate_version;
use std::env;
mod config;
mod providers;
mod shell;

use clap::Parser;
use shell::Shell;

use config::LadeFile;

#[derive(Subcommand, Debug)]
pub enum SelfCommand {
    /// Upgrade to the latest version of lade.
    Upgrade,
}

#[derive(Parser, Debug)]
pub struct EvalCommand {
    #[clap(trailing_var_arg = true, allow_hyphen_values = true)]
    commands: Vec<String>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Set of subcommands for lade itself.
    #[clap(subcommand)]
    _Self(SelfCommand),
    /// Enable execution hooks.
    On,
    /// Disable execution hooks.
    Off,
    /// Set environment for shell.
    Set(EvalCommand),
    /// Unset environment for shell.
    Unset(EvalCommand),
}

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    #[clap(subcommand)]
    command: Command,

    #[command(flatten)]
    verbose: Verbosity,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let current_dir = env::current_dir()?;
    let config = LadeFile::build(current_dir)?;
    let shell = Shell::from_env()?;

    env_logger::Builder::new()
        .filter_level(args.verbose.log_level_filter())
        .init();

    match args.command {
        Command::_Self(SelfCommand::Upgrade) => {
            let status = self_update::backends::github::Update::configure()
                .repo_owner("zifeo")
                .repo_name("lade")
                .bin_name("lade")
                .show_download_progress(true)
                .current_version(cargo_crate_version!())
                .build()?
                .update()?;
            println!("Update status: `{}`!", status.version());
            Ok(())
        }
        Command::Set(EvalCommand { commands }) => {
            let command = commands.join(" ");
            let vars = config.collect_hydrate(command).await?;
            println!("{}", shell.set(vars));
            Ok(())
        }
        Command::Unset(EvalCommand { commands }) => {
            let command = commands.join(" ");
            let vars = config.collect_keys(command)?;
            println!("{}", shell.unset(vars));
            Ok(())
        }
        Command::On => {
            println!("{};{}", shell.off(), shell.on());
            Ok(())
        }
        Command::Off => {
            println!("{}", shell.off());
            Ok(())
        }
    }
}

#[test]
fn verify_cli() {
    use crate::Args;
    use clap::CommandFactory;
    Args::command().debug_assert()
}

#[test]
fn end_to_end() {
    // need build before running this test
    use assert_cmd::Command;

    let mut cmd = Command::cargo_bin("lade").unwrap();
    cmd.arg("-h").assert().success();
}
