use anyhow::{Result, bail};
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum HookScope {
    User,
    Project,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum HookAgent {
    Claude,
    Cursor,
    Codex,
    #[value(name = "opencode")]
    OpenCode,
}

impl HookAgent {
    pub fn slug(self) -> &'static str {
        match self {
            HookAgent::Claude => "claude",
            HookAgent::Cursor => "cursor",
            HookAgent::Codex => "codex",
            HookAgent::OpenCode => "opencode",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookTarget {
    Shell,
    Agent(HookAgent),
}

#[derive(Parser, Debug)]
pub struct HookToggleCommand {
    /// This shell's pre-exec (machine).
    #[clap(long, conflicts_with = "agent")]
    pub shell: bool,
    /// `claude`, `cursor`, `codex`, or `opencode`.
    #[clap(long = "harness", conflicts_with = "shell")]
    pub agent: Option<HookAgent>,
    /// `project` is this repo (default). `user` is leftover home agent hooks.
    #[clap(long, default_value = "project")]
    pub scope: HookScope,
}

impl HookToggleCommand {
    pub fn target(&self) -> Result<HookTarget> {
        match (self.shell, self.agent) {
            (true, None) => Ok(HookTarget::Shell),
            (false, Some(agent)) => Ok(HookTarget::Agent(agent)),
            _ => bail!("pass --shell or --harness <slug>"),
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum HookAction {
    /// Write this shell's pre-exec, or one harness pre-tool hook.
    Enable(HookToggleCommand),
    /// Remove this shell's pre-exec, or one Lade-managed pre-tool hook.
    Disable(HookToggleCommand),
}
