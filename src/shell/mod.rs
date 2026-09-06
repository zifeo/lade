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
pub const LADE_T: &str = "LADE_T";

/// Protocol env the child must not inherit. `LADE_APPROVE` stays.
/// Stale names stay here so an upgraded binary still strips leftovers.
pub const CHILD_UNSET: [&str; 6] = [
    LADE_VIA,
    LADE_T,
    LADE_PENDING,
    LADE_RESTORE,
    LADE_NETWORK_PIDS,
    LADE_DISCLAIMER_APPROVED,
];

/// Dropped protocol keys. `set` / `unset` still clear them in the shell.
pub const STALE_UNSET: [&str; 4] = [
    LADE_PENDING,
    LADE_NETWORK_PIDS,
    LADE_VIA,
    LADE_DISCLAIMER_APPROVED,
];

pub fn strip_child_protocol(cmd: &mut std::process::Command) {
    for key in CHILD_UNSET {
        cmd.env_remove(key);
    }
}

pub fn strip_child_protocol_tokio(cmd: &mut tokio::process::Command) {
    for key in CHILD_UNSET {
        cmd.env_remove(key);
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
mod tests;
