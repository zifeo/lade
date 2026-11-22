use std::path::PathBuf;

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

pub(crate) const MARKER: &str = "lade-do-not-edit";

pub fn profile_config_file(shell: &Shell) -> PathBuf {
    let home_dir = crate::home_dir().expect("cannot get HOME location");
    match shell {
        Shell::Bash => home_dir.join(".bashrc"),
        Shell::Zsh => home_dir.join(".zshrc"),
        Shell::Fish => home_dir.join(".config/fish/config.fish"),
        _ => home_dir.join(".profile"),
    }
}

pub fn preexec_installed(shell: &Shell) -> (PathBuf, bool) {
    let path = profile_config_file(shell);
    let installed = profile_hooked(&path).unwrap_or(false);
    (path, installed)
}

/// Whether the profile holds Lade's line. A missing profile holds none; one
/// Lade cannot read gives no answer, so setup writes no second hook.
fn profile_hooked(path: &std::path::Path) -> Result<bool> {
    // § h528pmcm2.a@45wcw
    match std::fs::read_to_string(path) {
        Ok(content) => Ok(content.contains(MARKER)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => bail!(
            "Lade cannot read {}: {e}. It left the file as it is.",
            super::profile::path_for_display(path)
        ),
    }
}

impl Shell {
    pub fn on(&self) -> Result<String> {
        // § mqnfssecp.a@x1nf9
        let body = match self {
            Shell::Bash => format!(
                "{}\n{}",
                import!("../../scripts/bash-preexec.sh"),
                import!("../../scripts/on.bash")
            ),
            Shell::Zsh => import!("../../scripts/on.zsh"),
            Shell::Fish => import!("../../scripts/on.fish"),
            _ => bail!("Unsupported behavior on shell {}", self.bin()),
        };
        Ok(body)
    }

    pub fn off(&self) -> Result<String> {
        let body = match self {
            Shell::Bash => import!("../../scripts/off.bash"),
            Shell::Zsh => import!("../../scripts/off.zsh"),
            Shell::Fish => import!("../../scripts/off.fish"),
            _ => bail!("Unsupported behavior on shell {}", self.bin()),
        };
        Ok(body)
    }

    pub fn install(&self) -> Result<String> {
        super::profile::configure_auto_launch(self, true)
            .map(|c| super::profile::path_for_display(&c))
    }

    pub fn uninstall(&self) -> Result<String> {
        super::profile::configure_auto_launch(self, false)
            .map(|c| super::profile::path_for_display(&c))
    }
}

const DETECTABLE: [Shell; 3] = [Shell::Bash, Shell::Zsh, Shell::Fish];

pub fn present_shells() -> Vec<Shell> {
    // § mqnfssecp.a@mfpcs
    DETECTABLE
        .into_iter()
        .filter(|shell| profile_config_file(shell).is_file())
        .collect()
}

#[cfg(test)]
fn found_shells_line(found: &[Shell], current: Shell) -> String {
    let mut names: Vec<&str> = found.iter().map(|shell| shell.display_name()).collect();
    if !found.contains(&current) && !matches!(current, Shell::Sh) {
        names.push(current.display_name());
    }
    if names.is_empty() {
        names.push(current.display_name());
    }
    format!(
        "Found {}. This shell: {}.",
        names.join(", "),
        current.display_name()
    )
}

pub struct PreexecReport {
    pub verb: &'static str,
    pub path: String,
}

pub enum SetupShell {
    Bootstrapped {
        path: String,
        reload: String,
    },
    Current {
        path: String,
    },
    Missing,
    SkippedCi,
    Unknown(String),
    /// A profile Lade cannot read, so it cannot tell whether the machine
    /// has its hook.
    Unreadable(String),
}

pub fn ci_job() -> bool {
    // § h528pmcm2.a@x2thq
    std::env::var("CI")
        .map(|value| {
            let value = value.trim();
            !value.is_empty() && value != "0" && !value.eq_ignore_ascii_case("false")
        })
        .unwrap_or(false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BootstrapDecision {
    Current,
    Missing,
    Write,
}

pub(crate) fn bootstrap_decision(this_wrapped: bool, any_wrapped: bool) -> BootstrapDecision {
    // § h528pmcm2.a@pewzp
    if this_wrapped {
        return BootstrapDecision::Current;
    }
    if any_wrapped {
        return BootstrapDecision::Missing;
    }
    BootstrapDecision::Write
}

pub fn reload_hint(shell: Shell, path: &str) -> String {
    match shell {
        Shell::Bash | Shell::Zsh | Shell::Fish => format!("Reload this shell: source {path}"),
        Shell::Sh => "Open a new terminal to load the wrap.".to_string(),
    }
}

pub(crate) fn wrap_reload_command(shell: Shell, bin: &str) -> String {
    match shell {
        Shell::Bash => format!("source <(echo \"$({bin} on)\")"),
        Shell::Zsh | Shell::Sh => format!("eval \"$({bin} on)\""),
        Shell::Fish => format!("source ({bin} on | psub)"),
    }
}

pub(crate) fn reload_after_upgrade_lines(
    shell: Option<Shell>,
    bin: &str,
    wrapped: bool,
) -> Vec<String> {
    match (shell, wrapped) {
        (Some(shell), true) => vec![
            "This terminal still has the previous wrap.".to_string(),
            format!("Reload it: {}", wrap_reload_command(shell, bin)),
            "Or open a new terminal.".to_string(),
        ],
        _ => vec![
            "This terminal still has the previous binary.".to_string(),
            "Open a new terminal.".to_string(),
        ],
    }
}

/// A profile with Lade's line answers for the machine, whatever another
/// profile reads; with none, one Lade cannot read leaves no answer.
fn any_profile_wrapped() -> Result<bool> {
    // § h528pmcm2.a@85wd0
    let mut unread = None;
    for shell in DETECTABLE {
        match profile_hooked(&profile_config_file(&shell)) {
            Ok(true) => return Ok(true),
            Ok(false) => {}
            Err(error) => unread = unread.or(Some(error)),
        }
    }
    unread.map_or(Ok(false), Err)
}

/// First machine profile with no wrap: write this shell. Later setups
/// never write. A missing wrap is `lade hook enable --shell`.
pub fn maybe_bootstrap_setup_shell() -> Result<SetupShell> {
    // A CI step's parent may be no shell Lade knows.
    // § h528pmcm2.a@6d32v
    if ci_job() {
        return Ok(SetupShell::SkippedCi);
    }
    // The repo's steps need no shell, so setup names this one and goes on.
    let current = match Shell::detect() {
        Ok(shell) if DETECTABLE.contains(&shell) => shell,
        Ok(shell) => {
            return Ok(SetupShell::Unknown(format!(
                "Lade has no profile hook for {}",
                shell.bin()
            )));
        }
        Err(error) => return Ok(SetupShell::Unknown(format!("{error:#}"))),
    };
    let path_buf = profile_config_file(&current);
    let path = super::profile::path_for_display(&path_buf);
    let decision = profile_hooked(&path_buf)
        .and_then(|this| Ok(bootstrap_decision(this, this || any_profile_wrapped()?)));
    let decision = match decision {
        Ok(decision) => decision,
        Err(error) => return Ok(SetupShell::Unreadable(format!("{error:#}"))),
    };
    match decision {
        BootstrapDecision::Current => Ok(SetupShell::Current { path }),
        BootstrapDecision::Missing => Ok(SetupShell::Missing),
        BootstrapDecision::Write => {
            let path = current.install()?;
            Ok(SetupShell::Bootstrapped {
                path: path.clone(),
                reload: reload_hint(current, &path),
            })
        }
    }
}

pub fn enable_current_preexec() -> Result<(PreexecReport, Option<String>)> {
    let current = Shell::detect()?;
    let already = preexec_installed(&current).1;
    let path = current.install()?;
    let reload = (!already).then(|| reload_hint(current, &path));
    Ok((
        PreexecReport {
            verb: if already { "current" } else { "installed" },
            path,
        },
        reload,
    ))
}

pub fn uninstall_current_preexec() -> Result<PreexecReport> {
    let current = Shell::detect()?;
    let path = current.uninstall()?;
    Ok(PreexecReport {
        verb: "removed",
        path,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn found_shells_line_lists_detected_and_keeps_current_only() {
        assert_eq!(
            found_shells_line(&[Shell::Bash, Shell::Fish], Shell::Fish),
            "Found Bash, Fish. This shell: Fish."
        );
        assert_eq!(
            found_shells_line(&[Shell::Zsh], Shell::Fish),
            "Found Zsh, Fish. This shell: Fish."
        );
        assert_eq!(
            found_shells_line(&[], Shell::Zsh),
            "Found Zsh. This shell: Zsh."
        );
    }

    #[test]
    fn ci_skips_shell_wrap_before_reading_the_parent_shell() {
        // The test runner's parent is no shell Lade knows.
        temp_env::with_var("CI", Some("true"), || {
            assert!(matches!(
                maybe_bootstrap_setup_shell().unwrap(),
                SetupShell::SkippedCi
            ));
        });
    }

    #[test]
    fn first_profile_writes_later_warns() {
        assert_eq!(bootstrap_decision(false, false), BootstrapDecision::Write);
        assert_eq!(bootstrap_decision(true, false), BootstrapDecision::Current);
        assert_eq!(bootstrap_decision(false, true), BootstrapDecision::Missing);
    }

    #[test]
    fn ci_env_truthy_values() {
        temp_env::with_var("CI", Some("true"), || assert!(ci_job()));
        temp_env::with_var("CI", Some("1"), || assert!(ci_job()));
        temp_env::with_var("CI", Some("false"), || assert!(!ci_job()));
        temp_env::with_var("CI", Some("0"), || assert!(!ci_job()));
        temp_env::with_var("CI", None::<&str>, || assert!(!ci_job()));
    }

    #[test]
    fn wrap_reload_command_matches_the_profile_line() {
        assert_eq!(
            wrap_reload_command(Shell::Zsh, "lade"),
            "eval \"$(lade on)\""
        );
        assert_eq!(
            wrap_reload_command(Shell::Bash, "/opt/lade"),
            "source <(echo \"$(/opt/lade on)\")"
        );
        assert_eq!(
            wrap_reload_command(Shell::Fish, "lade"),
            "source (lade on | psub)"
        );
    }

    #[test]
    fn reload_after_upgrade_suggests_on_when_wrapped() {
        assert_eq!(
            reload_after_upgrade_lines(Some(Shell::Zsh), "lade", true),
            [
                "This terminal still has the previous wrap.",
                "Reload it: eval \"$(lade on)\"",
                "Or open a new terminal.",
            ]
        );
        assert_eq!(
            reload_after_upgrade_lines(None, "lade", false),
            [
                "This terminal still has the previous binary.",
                "Open a new terminal.",
            ]
        );
    }

    #[test]
    fn reload_after_upgrade_prints_each_shells_wrap_line() {
        for (shell, line) in [
            (Shell::Bash, "source <(echo \"$(/opt/lade on)\")"),
            (Shell::Zsh, "eval \"$(/opt/lade on)\""),
            (Shell::Fish, "source (/opt/lade on | psub)"),
            (Shell::Sh, "eval \"$(/opt/lade on)\""),
        ] {
            let lines = reload_after_upgrade_lines(Some(shell), "/opt/lade", true);
            assert_eq!(lines[1], format!("Reload it: {line}"), "{shell:?}");
            assert_eq!(wrap_reload_command(shell, "/opt/lade"), line, "{shell:?}");
        }
    }
}
