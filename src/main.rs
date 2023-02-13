use anyhow::Result;
use clap::Subcommand;
use self_update::cargo_crate_version;
use std::{collections::HashMap, env};
mod config;
mod providers;
mod shell;
use clap::Parser;
use shell::Shell;

use config::Config;

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
    #[clap(subcommand)]
    On,
    /// Disable execution hooks.
    #[clap(subcommand)]
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
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let current_dir = env::current_dir()?;
    let envs = Config::build_envs(current_dir)?;
    let shell = Shell::from_env()?;

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
            let mut vars = HashMap::default();

            for (regex, env) in envs {
                if regex.is_match(&command) {
                    vars.extend(env);
                }
            }

            println!("Eval: {:?}", shell.set(vars));
            let resp = reqwest::get("https://httpbin.org/ip")
                .await?
                .json::<HashMap<String, String>>()
                .await?;
            println!("{:#?}", resp);
            Ok(())
        }
        Command::Unset(_) => Ok(()),
        Command::On => Ok(()),
        Command::Off => Ok(()),
    }
}
