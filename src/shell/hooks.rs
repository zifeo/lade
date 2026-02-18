use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use super::Shell;

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

const MARKER: &str = "lade-do-not-edit";

impl Shell {
    pub fn on(&self) -> Result<String> {
        match self {
            Shell::Bash => Ok(format!(
                "{}\n{}",
                import!("../../scripts/bash-preexec.sh"),
                import!("../../scripts/on.bash")
            )),
            Shell::Zsh => Ok(import!("../../scripts/on.zsh")),
            Shell::Fish => Ok(import!("../../scripts/on.fish")),
            _ => bail!("Unsupported behavior on shell {}", self.bin()),
        }
    }

    pub fn off(&self) -> Result<String> {
        match self {
            Shell::Bash => Ok(import!("../../scripts/off.bash")),
            Shell::Zsh => Ok(import!("../../scripts/off.zsh")),
            Shell::Fish => Ok(import!("../../scripts/off.fish")),
            _ => bail!("Unsupported behavior on shell {}", self.bin()),
        }
    }

    pub fn install(&self) -> Result<String> {
        configure_auto_launch(self, true).map(|c| c.display().to_string())
    }

    pub fn uninstall(&self) -> Result<String> {
        configure_auto_launch(self, false).map(|c| c.display().to_string())
    }
}

fn configure_auto_launch(shell: &Shell, install: bool) -> Result<PathBuf> {
    let user = directories::UserDirs::new().expect("cannot get HOME location");
    let home_dir = user.home_dir();
    let curr_exe = std::env::current_exe()?;

    let (command, config_file) = match shell {
        Shell::Bash => (
            format!("source <(echo \"$({} on)\")", curr_exe.display()),
            home_dir.join(".bashrc"),
        ),
        Shell::Zsh => (
            format!("eval \"$({} on)\"", curr_exe.display()),
            home_dir.join(".zshrc"),
        ),
        Shell::Fish => (
            format!("source ({} on | psub)", curr_exe.display()),
            home_dir.join(".config/fish/config.fish"),
        ),
        _ => bail!("Unsupported behavior on shell {}", shell.bin()),
    };

    edit_config(&config_file, command, install)?;
    Ok(config_file)
}

fn edit_config<P: AsRef<Path>>(config_file: P, line: String, install: bool) -> Result<()> {
    let old_config = std::fs::read_to_string(&config_file).unwrap_or_default();
    let mut new_config = old_config
        .lines()
        .filter(|l| !l.contains(MARKER))
        .collect::<Vec<_>>();
    let new_line = format!("{}  # {}", line, MARKER);
    if install {
        new_config.push(&new_line);
    }
    std::fs::write(config_file, new_config.join("\n"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_edit_config_install_appends_line() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".bashrc");
        std::fs::write(&cfg, "existing content\n").unwrap();
        edit_config(&cfg, "eval $(lade on)".to_string(), true).unwrap();
        let content = std::fs::read_to_string(&cfg).unwrap();
        assert!(content.contains("eval $(lade on)"));
        assert!(content.contains(MARKER));
        assert!(content.contains("existing content"));
    }

    #[test]
    fn test_edit_config_install_idempotent() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".bashrc");
        std::fs::write(&cfg, "").unwrap();
        edit_config(&cfg, "eval $(lade on)".to_string(), true).unwrap();
        edit_config(&cfg, "eval $(lade on)".to_string(), true).unwrap();
        let content = std::fs::read_to_string(&cfg).unwrap();
        assert_eq!(content.lines().filter(|l| l.contains(MARKER)).count(), 1);
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
        edit_config(&cfg, "eval $(lade on)".to_string(), false).unwrap();
        let content = std::fs::read_to_string(&cfg).unwrap();
        assert!(!content.contains(MARKER));
        assert!(content.contains("other line") && content.contains("more content"));
    }

    #[test]
    fn test_edit_config_uninstall_no_marker_is_noop() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".bashrc");
        std::fs::write(&cfg, "line1\nline2").unwrap();
        edit_config(&cfg, "eval $(lade on)".to_string(), false).unwrap();
        let content = std::fs::read_to_string(&cfg).unwrap();
        assert!(content.contains("line1") && content.contains("line2"));
        assert!(!content.contains(MARKER));
    }
}
