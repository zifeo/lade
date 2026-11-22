use std::path::{Path, PathBuf};

use anyhow::{Result, bail};

use super::Shell;
use super::hook::{MARKER, profile_config_file};

pub(super) fn configure_auto_launch(shell: &Shell, install: bool) -> Result<PathBuf> {
    if matches!(shell, Shell::Sh) {
        // § mqnfssecp.a@y2zcy
        bail!("Unsupported behavior on shell {}", shell.bin());
    }
    // § aq4rxhrqa.a@e1pgw
    let command = super::wrap_reload_command(*shell, "lade");
    let config_file = profile_config_file(shell);

    edit_config(&config_file, command, install)?;
    Ok(config_file)
}

pub(crate) fn path_for_display(path: &Path) -> String {
    let Some(home) = crate::home_dir() else {
        return path.display().to_string();
    };
    match path.strip_prefix(&home) {
        Ok(stripped) if stripped.as_os_str().is_empty() => "~".to_string(),
        Ok(stripped) => format!("~/{}", stripped.display()),
        Err(_) => path.display().to_string(),
    }
}

fn edit_config<P: AsRef<Path>>(config_file: P, line: String, install: bool) -> Result<()> {
    let config_file = config_file.as_ref();
    // § aq4rxhrqa.c@jx9g7
    let old_config = match std::fs::read_to_string(config_file) {
        Ok(content) => content,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => bail!(
            "Lade cannot read {}: {e}. It left the file as it is.",
            path_for_display(config_file)
        ),
    };
    // § end aq4rxhrqa.c
    // § aq4rxhrqa.b@zp3tt
    let mut new_config = old_config
        .lines()
        .filter(|l| !l.contains(MARKER))
        .collect::<Vec<_>>();
    let new_line = format!("{}  # {}", line, MARKER);
    if install {
        new_config.push(&new_line);
    }
    let mut content = new_config.join("\n");
    if !content.is_empty() {
        content.push('\n');
    }
    // § end aq4rxhrqa.b
    std::fs::write(config_file, content)?;
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
    fn a_profile_lade_cannot_read_fails_and_stays_as_it_is() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".bashrc");
        let bytes = b"alias ll='ls -l'\n\xff\xfe not utf-8\n".to_vec();
        std::fs::write(&cfg, &bytes).unwrap();
        for install in [true, false] {
            let err = edit_config(&cfg, "eval $(lade on)".to_string(), install).unwrap_err();
            assert!(err.to_string().contains(".bashrc"), "{err}");
            assert_eq!(std::fs::read(&cfg).unwrap(), bytes);
        }
    }

    #[test]
    fn editing_the_profile_keeps_its_other_lines_and_last_newline() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".bashrc");
        std::fs::write(&cfg, "line1\nline2\n").unwrap();
        edit_config(&cfg, "eval $(lade on)".to_string(), true).unwrap();
        edit_config(&cfg, "eval $(lade on)".to_string(), false).unwrap();
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "line1\nline2\n");
    }

    #[test]
    fn writing_the_hook_keeps_the_line_the_installer_adds_to_path() {
        let installer = include_str!("../../installer.sh");
        let installer_marker = installer
            .lines()
            .find_map(|l| l.strip_prefix("PATH_MARKER=\"")?.strip_suffix('"'))
            .expect("installer.sh sets PATH_MARKER");
        assert!(!installer_marker.contains(MARKER), "{installer_marker}");
        let dir = tempdir().unwrap();
        let cfg = dir.path().join(".zshrc");
        let path_line = format!("export PATH=\"$PATH:/opt/lade\"  # {installer_marker}");
        std::fs::write(&cfg, format!("{path_line}\n")).unwrap();
        edit_config(&cfg, "eval \"$(lade on)\"".to_string(), true).unwrap();
        let content = std::fs::read_to_string(&cfg).unwrap();
        assert!(content.lines().any(|l| l == path_line), "{content}");
        assert_eq!(content.lines().filter(|l| l.contains(MARKER)).count(), 1);
        edit_config(&cfg, "eval \"$(lade on)\"".to_string(), false).unwrap();
        assert_eq!(
            std::fs::read_to_string(&cfg).unwrap(),
            format!("{path_line}\n")
        );
    }

    #[test]
    fn test_path_for_display_under_home() {
        if let Some(home) = crate::home_dir() {
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
