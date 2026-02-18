use anyhow::{Result, bail};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use sysinfo::{System, get_current_pid};

#[cfg(debug_assertions)]
macro_rules! import {
    ($x:expr) => {
        include_str!($x)
            .replace("lade set", "cargo run -- set")
            .replace("lade unset", "cargo run -- unset")
    };
}

#[cfg(not(debug_assertions))]
macro_rules! import {
    ($x:expr) => {
        include_str!($x).to_string()
    };
}

pub enum Shell {
    Bash,
    Zsh,
    Fish,
    Sh,
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
            // Accept either a plain name ("bash") or a full path ("/bin/bash")
            let path = std::path::Path::new(&shell_env);
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(shell_env.as_str());
            return match name {
                "bash" => Ok(Shell::Bash),
                "zsh" => Ok(Shell::Zsh),
                "fish" => Ok(Shell::Fish),
                "sh" => Ok(Shell::Sh),
                _ => bail!("Unsupported shell: {name}"),
            };
        }

        let sys = System::new_all();
        let process = sys
            .process(get_current_pid().expect("no pid"))
            .expect("pid does not exist");
        let parent = sys
            .process(process.parent().expect("no parent pid"))
            .expect("parent pid does not exist");
        let shell = parent.name().to_string_lossy().trim().to_lowercase();
        let shell = shell.strip_suffix(".exe").unwrap_or(&shell); // windows bad

        match shell {
            "bash" => Ok(Shell::Bash),
            "zsh" => Ok(Shell::Zsh),
            "fish" => Ok(Shell::Fish),
            "sh" => Ok(Shell::Sh),
            _ => bail!("Unsupported shell: {shell}"),
        }
    }

    pub fn on(&self) -> Result<String> {
        match self {
            Shell::Bash => Ok(format!(
                "{}\n{}",
                import!("../scripts/bash-preexec.sh"),
                import!("../scripts/on.bash")
            )),
            Shell::Zsh => Ok(import!("../scripts/on.zsh")),
            Shell::Fish => Ok(import!("../scripts/on.fish")),
            _ => {
                let shell = self.bin();
                bail!("Unsupported behavior on shell {shell}")
            }
        }
    }

    pub fn off(&self) -> Result<String> {
        match self {
            Shell::Bash => Ok(import!("../scripts/off.bash")),
            Shell::Zsh => Ok(import!("../scripts/off.zsh")),
            Shell::Fish => Ok(import!("../scripts/off.fish")),
            _ => {
                let shell = self.bin();
                bail!("Unsupported behavior on shell {shell}")
            }
        }
    }

    pub fn set(&self, env: HashMap<String, String>) -> String {
        env.into_iter()
            .map(|(k, v)| match self {
                Shell::Bash | Shell::Zsh | Shell::Sh => {
                    format!("export {k}='{v}'")
                }
                Shell::Fish => {
                    format!("set --global --export {k} '{v}'")
                }
            })
            .collect::<Vec<_>>()
            .join(";")
    }

    pub fn unset(&self, keys: Vec<String>) -> String {
        let format = match self {
            Shell::Zsh | Shell::Bash | Shell::Sh => |k| format!("unset -v {k}"),
            Shell::Fish => |k| format!("set --global --erase {k}"),
        };
        keys.into_iter().map(format).collect::<Vec<_>>().join(";")
    }

    pub fn install(&self) -> Result<String> {
        self.configure_auto_launch(true)
            .map(|c| c.display().to_string())
    }

    pub fn uninstall(&self) -> Result<String> {
        self.configure_auto_launch(false)
            .map(|c| c.display().to_string())
    }

    fn configure_auto_launch(&self, install: bool) -> Result<PathBuf> {
        let user = directories::UserDirs::new().expect("cannot get HOME location");
        let home_dir = user.home_dir();
        let curr_exe = std::env::current_exe().expect("cannot get current executable path");
        let command = match self {
            Shell::Bash => format!("source <(echo \"$({} on)\")", curr_exe.display()),
            Shell::Zsh => format!("eval \"$({} on)\"", curr_exe.display()),
            Shell::Fish => format!("source ({} on | psub)", curr_exe.display()),
            _ => {
                let shell = self.bin();
                bail!("Unsupported behavior on shell {shell}")
            }
        };
        let marker = "lade-do-not-edit".to_string();
        let config_file = match self {
            Shell::Bash => home_dir.join(".bashrc"),
            Shell::Zsh => home_dir.join(".zshrc"),
            Shell::Fish => home_dir.join(".config/fish/config.fish"),
            _ => {
                let shell = self.bin();
                bail!("Unsupported behavior on shell {shell}")
            }
        };
        edit_config(&config_file, command, marker, install);
        Ok(config_file)
    }
}

fn edit_config<P: AsRef<Path>>(config_file: P, line: String, marker: String, install: bool) {
    let old_config = std::fs::read_to_string(&config_file).unwrap_or_default();
    let mut new_config = old_config
        .lines()
        .filter(|l| !l.contains(&marker))
        .collect::<Vec<_>>();
    let new_line = format!("{}  # {}", line, marker);
    if install {
        new_config.push(&new_line);
    }
    std::fs::write(config_file, new_config.join("\n")).expect("cannot write to config file");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::tempdir;

    // --- Shell::set ---

    #[test]
    fn test_set_bash_single_key() {
        let result = Shell::Bash.set(HashMap::from([("KEY".to_string(), "value".to_string())]));
        assert_eq!(result, "export KEY='value'");
    }

    #[test]
    fn test_set_zsh_single_key() {
        let result = Shell::Zsh.set(HashMap::from([("KEY".to_string(), "value".to_string())]));
        assert_eq!(result, "export KEY='value'");
    }

    #[test]
    fn test_set_fish_single_key() {
        let result = Shell::Fish.set(HashMap::from([("KEY".to_string(), "value".to_string())]));
        assert_eq!(result, "set --global --export KEY 'value'");
    }

    #[test]
    fn test_set_empty_map() {
        assert_eq!(Shell::Bash.set(HashMap::new()), "");
    }

    #[test]
    fn test_set_multiple_keys_contains() {
        let env = HashMap::from([
            ("A".to_string(), "1".to_string()),
            ("B".to_string(), "2".to_string()),
        ]);
        let result = Shell::Bash.set(env);
        assert!(result.contains("export A='1'"));
        assert!(result.contains("export B='2'"));
        assert!(result.contains(';'));
    }

    // --- Shell::unset ---

    #[test]
    fn test_unset_bash_single_key() {
        let result = Shell::Bash.unset(vec!["KEY".to_string()]);
        assert_eq!(result, "unset -v KEY");
    }

    #[test]
    fn test_unset_fish_single_key() {
        let result = Shell::Fish.unset(vec!["KEY".to_string()]);
        assert_eq!(result, "set --global --erase KEY");
    }

    #[test]
    fn test_unset_multiple_keys_order_preserved() {
        let result = Shell::Bash.unset(vec!["KEY1".to_string(), "KEY2".to_string()]);
        assert_eq!(result, "unset -v KEY1;unset -v KEY2");
    }

    // --- edit_config ---

    #[test]
    fn test_edit_config_install_appends_line() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".bashrc");
        std::fs::write(&cfg, "existing content\n").unwrap();
        edit_config(
            &cfg,
            "eval $(lade on)".to_string(),
            "lade-do-not-edit".to_string(),
            true,
        );
        let content = std::fs::read_to_string(&cfg).unwrap();
        assert!(content.contains("eval $(lade on)"));
        assert!(content.contains("lade-do-not-edit"));
        assert!(content.contains("existing content"));
    }

    #[test]
    fn test_edit_config_install_idempotent() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".bashrc");
        std::fs::write(&cfg, "").unwrap();
        edit_config(
            &cfg,
            "eval $(lade on)".to_string(),
            "lade-do-not-edit".to_string(),
            true,
        );
        edit_config(
            &cfg,
            "eval $(lade on)".to_string(),
            "lade-do-not-edit".to_string(),
            true,
        );
        let content = std::fs::read_to_string(&cfg).unwrap();
        let count = content
            .lines()
            .filter(|l| l.contains("lade-do-not-edit"))
            .count();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_edit_config_uninstall_removes_marker_line() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".bashrc");
        std::fs::write(
            &cfg,
            "other line\neval $(lade on)  # lade-do-not-edit\nmore content",
        )
        .unwrap();
        edit_config(
            &cfg,
            "eval $(lade on)".to_string(),
            "lade-do-not-edit".to_string(),
            false,
        );
        let content = std::fs::read_to_string(&cfg).unwrap();
        assert!(!content.contains("lade-do-not-edit"));
        assert!(content.contains("other line"));
        assert!(content.contains("more content"));
    }

    #[test]
    fn test_edit_config_uninstall_no_marker_is_noop() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".bashrc");
        std::fs::write(&cfg, "line1\nline2").unwrap();
        edit_config(
            &cfg,
            "eval $(lade on)".to_string(),
            "lade-do-not-edit".to_string(),
            false,
        );
        let content = std::fs::read_to_string(&cfg).unwrap();
        assert!(content.contains("line1"));
        assert!(content.contains("line2"));
        assert!(!content.contains("lade-do-not-edit"));
    }
}
