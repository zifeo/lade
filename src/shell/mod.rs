mod preexec;

pub use preexec::preexec_installed;

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{collections::HashMap, path::PathBuf, str::FromStr};

pub const LADE_PENDING: &str = "LADE_PENDING";
pub const LADE_DISCLAIMER_APPROVED: &str = "LADE_DISCLAIMER_APPROVED";
pub const LADE_APPROVE: &str = "LADE_APPROVE";
pub const LADE_NETWORK_PIDS: &str = "LADE_NETWORK_PIDS";
pub const LADE_RESTORE: &str = "LADE_RESTORE";
pub const LADE_VIA: &str = "LADE_VIA";
pub const LADE_VIA_PREEXEC: &str = "preexec";
pub const LADE_VIA_PRETOOL: &str = "pretool";

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PendingPayload {
    pub cmd: String,
    pub cwd: PathBuf,
}

impl PendingPayload {
    pub fn encode(&self) -> Result<String> {
        encode_v1("LADE_PENDING", self)
    }

    pub fn decode(value: &str) -> Result<Self> {
        decode_v1("LADE_PENDING", value)
    }
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RestorePayload {
    pub env: HashMap<String, Option<String>>,
}

impl RestorePayload {
    pub fn encode(&self) -> Result<String> {
        encode_v1("LADE_RESTORE", self)
    }

    pub fn decode(value: &str) -> Result<Self> {
        decode_v1("LADE_RESTORE", value)
    }
}

fn encode_v1<T: Serialize>(label: &str, value: &T) -> Result<String> {
    let json = serde_json::to_string(value).with_context(|| format!("failed to encode {label}"))?;
    Ok(format!("v1:{}", URL_SAFE_NO_PAD.encode(json)))
}

fn decode_v1<T: DeserializeOwned>(label: &str, value: &str) -> Result<T> {
    let encoded = value
        .strip_prefix("v1:")
        .with_context(|| format!("invalid or unsupported {label} version"))?;
    let json = URL_SAFE_NO_PAD
        .decode(encoded)
        .with_context(|| format!("failed to decode {label} base64"))?;
    serde_json::from_slice(&json).with_context(|| format!("failed to parse {label} JSON"))
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

    /// Argv after the binary for a non-interactive `-c` that must not source
    /// user startup files. `fish -c` reads `config.fish` and `zsh -c` reads
    /// `.zshenv`; either can overwrite resolved secrets after process env is
    /// applied. `bash -c` does not read `.bashrc` / `.bash_profile`; its
    /// overwrite vector is `$BASH_ENV`, cleared on the child `Command`.
    /// `--norc --noprofile` are omitted: they do not skip `BASH_ENV`, and
    /// `bash -c` does not read `.bashrc` or login profiles anyway. Preexec
    /// does not use this: it evals `set()` in the already-running
    /// interactive shell.
    pub fn noninteractive_args<'a>(&self, command: &'a str) -> Vec<&'a str> {
        match self {
            Shell::Fish => vec!["--no-config", "-c", command],
            Shell::Zsh => vec!["-f", "-c", command],
            Shell::Bash | Shell::Sh => vec!["-c", command],
        }
    }

    pub fn prepare_command(&self, command: &str) -> std::process::Command {
        let mut cmd = std::process::Command::new(self.bin());
        cmd.args(self.noninteractive_args(command));
        cmd
    }

    /// Startup file `inject` skips when present. `None` for bash/sh: those
    /// `-c` invocations do not read a user file unless `$BASH_ENV` is set.
    pub fn wrap_startup_file(&self) -> Option<PathBuf> {
        let home = directories::UserDirs::new()?.home_dir().to_path_buf();
        match self {
            Shell::Fish => Some(home.join(".config/fish/config.fish")),
            Shell::Zsh => Some(home.join(".zshenv")),
            Shell::Bash | Shell::Sh => None,
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

        let name = parent_process_name()?;
        shell_from_comm(&name)
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

    pub fn restore(&self, previous: HashMap<String, Option<String>>) -> String {
        let mut set = HashMap::new();
        let mut unset = Vec::new();
        for (key, value) in previous {
            match value {
                Some(value) => {
                    set.insert(key, value);
                }
                None => unset.push(key),
            }
        }
        let mut parts = Vec::new();
        if !set.is_empty() {
            parts.push(self.set(set));
        }
        if !unset.is_empty() {
            parts.push(self.unset(unset));
        }
        parts.join(";")
    }
}

fn shell_from_comm(name: &str) -> Result<Shell> {
    let shell = name.trim().to_lowercase();
    let shell = shell.strip_suffix(".exe").unwrap_or(shell.as_str());
    let shell = std::path::Path::new(shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(shell);
    Shell::from_str(shell)
}

fn parent_process_name() -> Result<String> {
    #[cfg(not(unix))]
    {
        bail!("set LADE_SHELL; parent detection is Unix-only");
    }
    #[cfg(unix)]
    {
        parent_comm(nix::unistd::getppid())
    }
}

#[cfg(target_os = "linux")]
fn parent_comm(ppid: nix::unistd::Pid) -> Result<String> {
    let path = format!("/proc/{}/comm", ppid.as_raw());
    let name = std::fs::read_to_string(&path).with_context(|| format!("cannot read {path}"))?;
    Ok(name.trim().to_string())
}

#[cfg(target_os = "macos")]
fn parent_comm(ppid: nix::unistd::Pid) -> Result<String> {
    let mut buf = [0u8; 256];
    let n = unsafe {
        libc::proc_name(
            ppid.as_raw(),
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as u32,
        )
    };
    if n <= 0 {
        bail!("cannot read parent process name");
    }
    let nbytes = (n as usize).min(buf.len());
    let end = buf[..nbytes].iter().position(|&b| b == 0).unwrap_or(nbytes);
    String::from_utf8(buf[..end].to_vec()).context("parent process name is not utf-8")
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn parent_comm(_ppid: nix::unistd::Pid) -> Result<String> {
    bail!("set LADE_SHELL; parent detection is Linux/macOS-only");
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
    fn test_restore_replaces_and_erases() {
        let previous = HashMap::from([
            ("KEEP".to_string(), Some("sock".to_string())),
            ("DROP".to_string(), None),
        ]);
        let result = Shell::Bash.restore(previous);
        assert!(result.contains("export KEEP='sock'"));
        assert!(result.contains("unset -v DROP"));
    }

    #[test]
    fn test_restore_payload_roundtrip() {
        let payload = RestorePayload {
            env: HashMap::from([
                (
                    "SSH_AUTH_SOCK".to_string(),
                    Some("/tmp/agent.sock".to_string()),
                ),
                ("NEW".to_string(), None),
            ]),
        };
        let decoded = RestorePayload::decode(&payload.encode().unwrap()).unwrap();
        assert_eq!(payload, decoded);
        assert!(RestorePayload::decode("not-v1").is_err());
        assert!(RestorePayload::decode("v1:!!!").is_err());
    }

    #[test]
    fn test_restore_fish_syntax() {
        let previous = HashMap::from([
            ("KEEP".to_string(), Some("sock".to_string())),
            ("DROP".to_string(), None),
        ]);
        let result = Shell::Fish.restore(previous);
        assert!(result.contains("set --global --export KEEP 'sock'"));
        assert!(result.contains("set --global --erase DROP"));
    }

    #[test]
    fn test_set_escaping() {
        let env = HashMap::from([("KEY".to_string(), "val'ue".to_string())]);
        let result = Shell::Bash.set(env);
        assert_eq!(result, "export KEY='val'\\''ue'");
    }

    #[test]
    fn wrap_startup_file_is_shell_specific() {
        assert!(
            Shell::Fish
                .wrap_startup_file()
                .unwrap()
                .ends_with(".config/fish/config.fish")
        );
        assert!(Shell::Zsh.wrap_startup_file().unwrap().ends_with(".zshenv"));
        assert!(Shell::Bash.wrap_startup_file().is_none());
        assert!(Shell::Sh.wrap_startup_file().is_none());
    }

    #[test]
    fn noninteractive_args_skip_startup_files() {
        assert_eq!(
            Shell::Fish.noninteractive_args("echo hi"),
            vec!["--no-config", "-c", "echo hi"]
        );
        assert_eq!(
            Shell::Bash.noninteractive_args("echo hi"),
            vec!["-c", "echo hi"]
        );
        assert_eq!(
            Shell::Zsh.noninteractive_args("echo hi"),
            vec!["-f", "-c", "echo hi"]
        );
        assert_eq!(
            Shell::Sh.noninteractive_args("echo hi"),
            vec!["-c", "echo hi"]
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
