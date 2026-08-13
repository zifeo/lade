mod hooks;

pub use hooks::hook_installed;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, str::FromStr};
use sysinfo::{ProcessesToUpdate, System, get_current_pid};

pub const LADE_PENDING: &str = "LADE_PENDING";
pub const LADE_DISCLAIMER_APPROVED: &str = "LADE_DISCLAIMER_APPROVED";
pub const LADE_APPROVE: &str = "LADE_APPROVE";
pub const LADE_NETWORK_PIDS: &str = "LADE_NETWORK_PIDS";

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingPayload {
    pub cmd: String,
    pub cwd: PathBuf,
}

impl PendingPayload {
    pub fn encode(&self) -> Result<String> {
        let json = serde_json::to_string(self)?;
        Ok(format!("v1:{}", URL_SAFE_NO_PAD.encode(json)))
    }

    pub fn decode(value: &str) -> Result<Self> {
        let encoded = value
            .strip_prefix("v1:")
            .context("invalid or unsupported LADE_PENDING version")?;
        let json = URL_SAFE_NO_PAD
            .decode(encoded)
            .context("failed to decode LADE_PENDING base64")?;
        serde_json::from_slice(&json).context("failed to parse LADE_PENDING JSON")
    }
}

pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Sh,
}

impl FromStr for Shell {
    type Err = anyhow::Error;

    fn from_str(name: &str) -> Result<Self> {
        match name {
            "bash" => Ok(Shell::Bash),
            "zsh" => Ok(Shell::Zsh),
            "fish" => Ok(Shell::Fish),
            "sh" => Ok(Shell::Sh),
            _ => bail!("Unsupported shell: {name}"),
        }
    }
}

impl Shell {
    pub fn bin(&self) -> &str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::Sh => "sh",
        }
    }

    pub fn detect() -> Result<Shell> {
        if let Ok(shell_env) = std::env::var("LADE_SHELL") {
            let path = std::path::Path::new(&shell_env);
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(shell_env.as_str());
            return Shell::from_str(name);
        }

        let mut sys = System::new();
        let pid = get_current_pid().map_err(|e| anyhow::anyhow!("{e}"))?;
        // Only this process and its parent. `System::new_all()` also walks disks,
        // networks, and every pid, which can stall on a dead volume.
        sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), false);
        let process = sys.process(pid).context("pid does not exist")?;
        let parent_pid = process.parent().context("no parent pid")?;
        sys.refresh_processes(ProcessesToUpdate::Some(&[parent_pid]), false);
        let parent = sys
            .process(parent_pid)
            .context("parent pid does not exist")?;
        let shell = parent.name().to_string_lossy().trim().to_lowercase();
        let shell = shell.strip_suffix(".exe").unwrap_or(&shell);
        Shell::from_str(shell)
    }

    fn export_lade_shell(&self) -> String {
        match self {
            Shell::Bash | Shell::Zsh | Shell::Sh => {
                format!("export LADE_SHELL={}", self.bin())
            }
            Shell::Fish => format!("set --global --export LADE_SHELL {}", self.bin()),
        }
    }

    fn unset_lade_shell(&self) -> String {
        match self {
            Shell::Bash | Shell::Zsh | Shell::Sh => "unset -v LADE_SHELL".to_string(),
            Shell::Fish => "set --global --erase LADE_SHELL".to_string(),
        }
    }

    pub fn set(&self, env: HashMap<String, String>) -> String {
        env.into_iter()
            .map(|(k, v)| {
                let v = v.replace('\'', "'\\''");
                match self {
                    Shell::Bash | Shell::Zsh | Shell::Sh => format!("export {k}='{v}'"),
                    Shell::Fish => format!("set --global --export {k} '{v}'"),
                }
            })
            .collect::<Vec<_>>()
            .join(";")
    }

    pub fn unset(&self, keys: Vec<String>) -> String {
        let format = match self {
            Shell::Zsh | Shell::Bash | Shell::Sh => |k: String| format!("unset -v {k}"),
            Shell::Fish => |k: String| format!("set --global --erase {k}"),
        };
        keys.into_iter().map(format).collect::<Vec<_>>().join(";")
    }

    pub fn clear_pending_line(&self) -> String {
        self.unset(vec![LADE_PENDING.to_string()])
    }

    pub fn clear_network_line(&self) -> String {
        self.unset(vec![LADE_NETWORK_PIDS.to_string()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_set_bash_single_key() {
        let result = Shell::Bash.set(HashMap::from([("KEY".to_string(), "value".to_string())]));
        assert_eq!(result, "export KEY='value'");
    }

    #[test]
    fn test_set_zsh_single_key() {
        assert_eq!(
            Shell::Zsh.set(HashMap::from([("KEY".to_string(), "value".to_string())])),
            "export KEY='value'"
        );
    }

    #[test]
    fn test_set_fish_single_key() {
        assert_eq!(
            Shell::Fish.set(HashMap::from([("KEY".to_string(), "value".to_string())])),
            "set --global --export KEY 'value'"
        );
    }

    #[test]
    fn test_set_empty_map() {
        assert_eq!(Shell::Bash.set(HashMap::new()), "");
    }

    #[test]
    fn test_set_multiple_keys_contains() {
        let result = Shell::Bash.set(HashMap::from([
            ("A".to_string(), "1".to_string()),
            ("B".to_string(), "2".to_string()),
        ]));
        assert!(result.contains("export A='1'") && result.contains("export B='2'"));
        assert!(result.contains(';'));
    }

    #[test]
    fn test_unset_bash_single_key() {
        assert_eq!(Shell::Bash.unset(vec!["KEY".to_string()]), "unset -v KEY");
    }

    #[test]
    fn test_unset_fish_single_key() {
        assert_eq!(
            Shell::Fish.unset(vec!["KEY".to_string()]),
            "set --global --erase KEY"
        );
    }

    #[test]
    fn test_unset_multiple_keys_order_preserved() {
        assert_eq!(
            Shell::Bash.unset(vec!["KEY1".to_string(), "KEY2".to_string()]),
            "unset -v KEY1;unset -v KEY2"
        );
    }

    #[test]
    fn test_pending_payload_roundtrip() {
        let payload = PendingPayload {
            cmd: "terraform destroy -auto-approve".to_string(),
            cwd: PathBuf::from("/tmp/project"),
        };
        let encoded = payload.encode().unwrap();
        assert!(encoded.starts_with("v1:"));
        let decoded = PendingPayload::decode(&encoded).unwrap();
        assert_eq!(payload, decoded);
    }

    #[test]
    fn test_set_escaping() {
        let env = HashMap::from([("KEY".to_string(), "val'ue".to_string())]);
        let result = Shell::Bash.set(env);
        assert_eq!(result, "export KEY='val'\\''ue'");
    }

    #[test]
    fn test_clear_pending_line() {
        assert_eq!(Shell::Bash.clear_pending_line(), "unset -v LADE_PENDING");
        assert_eq!(
            Shell::Fish.clear_pending_line(),
            "set --global --erase LADE_PENDING"
        );
    }

    #[test]
    fn on_exports_lade_shell() {
        let zsh = Shell::Zsh.on().unwrap();
        assert!(zsh.starts_with("export LADE_SHELL=zsh\n"));
        let bash = Shell::Bash.on().unwrap();
        assert!(bash.starts_with("export LADE_SHELL=bash\n"));
        let fish = Shell::Fish.on().unwrap();
        assert!(fish.starts_with("set --global --export LADE_SHELL fish\n"));
    }

    #[test]
    fn off_unsets_lade_shell() {
        assert!(
            Shell::Zsh
                .off()
                .unwrap()
                .starts_with("unset -v LADE_SHELL\n")
        );
        assert!(
            Shell::Fish
                .off()
                .unwrap()
                .starts_with("set --global --erase LADE_SHELL\n")
        );
    }
}
