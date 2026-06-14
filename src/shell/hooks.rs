use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use super::Shell;

macro_rules! import {
    ($x:expr) => {
        include_str!($x).to_string()
    };
}

pub(crate) const MARKER: &str = "lade-do-not-edit";

pub fn profile_config_file(shell: &Shell) -> PathBuf {
    let user = directories::UserDirs::new().expect("cannot get HOME location");
    let home_dir = user.home_dir();
    match shell {
        Shell::Bash => home_dir.join(".bashrc"),
        Shell::Zsh => home_dir.join(".zshrc"),
        Shell::Fish => home_dir.join(".config/fish/config.fish"),
        _ => home_dir.join(".profile"),
    }
}

pub fn hook_installed(shell: &Shell) -> (PathBuf, bool) {
    let path = profile_config_file(shell);
    let installed = std::fs::read_to_string(&path)
        .map(|content| content.contains(MARKER))
        .unwrap_or(false);
    (path, installed)
}

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
        configure_auto_launch(self, true).map(|c| path_for_display(&c))
    }

    pub fn uninstall(&self) -> Result<String> {
        configure_auto_launch(self, false).map(|c| path_for_display(&c))
    }
}

fn configure_auto_launch(shell: &Shell, install: bool) -> Result<PathBuf> {
    let curr_exe = std::env::current_exe()?;

    let (command, config_file) = match shell {
        Shell::Bash => (
            format!("source <(echo \"$({} on)\")", curr_exe.display()),
            profile_config_file(shell),
        ),
        Shell::Zsh => (
            format!("eval \"$({} on)\"", curr_exe.display()),
            profile_config_file(shell),
        ),
        Shell::Fish => (
            format!("source ({} on | psub)", curr_exe.display()),
            profile_config_file(shell),
        ),
        _ => bail!("Unsupported behavior on shell {}", shell.bin()),
    };

    edit_config(&config_file, command, install)?;
    Ok(config_file)
}

fn path_for_display(path: &Path) -> String {
    let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()) else {
        return path.display().to_string();
    };
    match path.strip_prefix(&home) {
        Ok(stripped) if stripped.as_os_str().is_empty() => "~".to_string(),
        Ok(stripped) => format!("~/{}", stripped.display()),
        Err(_) => path.display().to_string(),
    }
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
    fn test_path_for_display_under_home() {
        if let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()) {
            let cfg = home.join(".zshrc");
            assert_eq!(path_for_display(&cfg), "~/.zshrc");
        }
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
