use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Agent {
    Cursor,
    Claude,
    Codex,
    OpenCode,
}

pub(super) const AGENTS: [Agent; 4] = [Agent::Cursor, Agent::Claude, Agent::Codex, Agent::OpenCode];

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var_os(key)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn codex_home(home: &Path) -> PathBuf {
    env_path("CODEX_HOME").unwrap_or_else(|| home.join(".codex"))
}

impl Agent {
    pub(super) fn name(self) -> &'static str {
        match self {
            Agent::Cursor => "Cursor",
            Agent::Claude => "Claude Code",
            Agent::Codex => "Codex",
            Agent::OpenCode => "OpenCode",
        }
    }

    pub(super) fn slug(self) -> &'static str {
        match self {
            Agent::Cursor => "cursor",
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::OpenCode => "opencode",
        }
    }

    pub(super) fn from_slug(slug: &str) -> Option<Self> {
        match slug {
            "cursor" => Some(Agent::Cursor),
            "claude" => Some(Agent::Claude),
            "codex" => Some(Agent::Codex),
            "opencode" => Some(Agent::OpenCode),
            _ => None,
        }
    }

    pub(super) fn config_path(self, home: &Path) -> PathBuf {
        match self {
            Agent::Cursor => home.join(".cursor").join("hooks.json"),
            Agent::Claude => home.join(".claude").join("settings.json"),
            Agent::Codex => codex_home(home).join("hooks.json"),
            Agent::OpenCode => home
                .join(".config")
                .join("opencode")
                .join("plugins")
                .join("lade-pretool.js"),
        }
    }

    pub(super) fn home_dir(self, home: &Path) -> PathBuf {
        match self {
            Agent::Cursor => home.join(".cursor"),
            Agent::Claude => home.join(".claude"),
            Agent::Codex => codex_home(home),
            Agent::OpenCode => home.join(".config").join("opencode"),
        }
    }

    pub(super) fn skill_path(self, home: &Path) -> PathBuf {
        self.home_dir(home)
            .join("skills")
            .join("lade")
            .join("SKILL.md")
    }

    /// Repo file this binary serves. Same model as the bundled `SKILL.md`.
    pub(super) fn snapshot(self) -> &'static str {
        match self {
            Agent::Cursor => include_str!("../../../.cursor/hooks.json"),
            Agent::Claude => include_str!("../../../.claude/settings.json"),
            Agent::Codex => include_str!("../../../.codex/hooks.json"),
            Agent::OpenCode => include_str!("../../../.opencode/plugins/lade-pretool.js"),
        }
    }

    pub(super) fn instantiate(self, command: &str) -> String {
        if matches!(self, Agent::OpenCode) {
            return instantiate_plugin(self.snapshot(), command);
        }
        instantiate_json(self.snapshot(), self.slug(), command)
    }

    /// Claude-compat `hooks.json` left by an older install. Native OpenCode
    /// ignores it; uninstall still strips our entry.
    pub(super) fn legacy_json_path(self, home: &Path) -> Option<PathBuf> {
        match self {
            Agent::OpenCode => Some(home.join(".config").join("opencode").join("hooks.json")),
            _ => None,
        }
    }
}

fn instantiate_json(snapshot: &str, slug: &str, command: &str) -> String {
    snapshot.replace(&format!("lade hook --harness {slug}"), command)
}

fn instantiate_plugin(snapshot: &str, command: &str) -> String {
    let bin = command
        .split_whitespace()
        .next()
        .unwrap_or("lade")
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    if bin == "lade" {
        return snapshot.to_string();
    }
    snapshot.replace(
        r#"process.env.LADE_BIN ?? "lade""#,
        &format!(r#"process.env.LADE_BIN ?? "{bin}""#),
    )
}
