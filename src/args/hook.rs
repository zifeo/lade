use clap::{Parser, Subcommand, ValueEnum};

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum HookScope {
    User,
    Project,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum HookHarness {
    Claude,
    Cursor,
    Codex,
    #[value(name = "opencode")]
    OpenCode,
}

impl HookHarness {
    pub fn slug(self) -> &'static str {
        match self {
            HookHarness::Claude => "claude",
            HookHarness::Cursor => "cursor",
            HookHarness::Codex => "codex",
            HookHarness::OpenCode => "opencode",
        }
    }
}

#[derive(Parser, Debug)]
pub struct HookScopeCommand {
    /// `project` is the repo (default). `user` is this machine.
    #[clap(long, default_value = "project")]
    pub scope: HookScope,
    /// `claude`, `cursor`, `codex`, or `opencode`.
    #[clap(long)]
    pub harness: HookHarness,
}

#[derive(Subcommand, Debug)]
pub enum HookAction {
    /// Write the native preTool hook for one harness.
    Install(HookScopeCommand),
    /// Remove a Lade-managed preTool hook.
    Uninstall(HookScopeCommand),
}
