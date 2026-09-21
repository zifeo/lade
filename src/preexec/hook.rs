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
    let user = directories::UserDirs::new().expect("cannot get HOME location");
    let home_dir = user.home_dir();
    match shell {
        Shell::Bash => home_dir.join(".bashrc"),
        Shell::Zsh => home_dir.join(".zshrc"),
        Shell::Fish => home_dir.join(".config/fish/config.fish"),
        _ => home_dir.join(".profile"),
    }
}

pub fn preexec_installed(shell: &Shell) -> (PathBuf, bool) {
    let path = profile_config_file(shell);
    let installed = std::fs::read_to_string(&path)
        .map(|content| content.contains(MARKER))
        .unwrap_or(false);
    (path, installed)
}

impl Shell {
    pub fn on(&self) -> Result<String> {
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
        Ok(format!("{}\n{}", self.export_lade_shell(), body))
    }

    pub fn off(&self) -> Result<String> {
        let body = match self {
            Shell::Bash => import!("../../scripts/off.bash"),
            Shell::Zsh => import!("../../scripts/off.zsh"),
            Shell::Fish => import!("../../scripts/off.fish"),
            _ => bail!("Unsupported behavior on shell {}", self.bin()),
        };
        Ok(format!("{}\n{}", self.unset_lade_shell(), body))
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
    Bootstrapped { path: String, reload: String },
    Current { path: String },
    Missing,
    SkippedCi,
}

pub fn ci_job() -> bool {
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
    SkipCi,
    Write,
}

pub(crate) fn bootstrap_decision(
    this_wrapped: bool,
    any_wrapped: bool,
    ci: bool,
) -> BootstrapDecision {
    if ci {
        return BootstrapDecision::SkipCi;
    }
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

pub fn any_profile_wrapped() -> bool {
    DETECTABLE.iter().any(|shell| preexec_installed(shell).1)
}

/// First machine profile with no wrap: write this shell. Later setups
/// never write. A missing wrap is `lade hook enable --shell`.
pub fn maybe_bootstrap_setup_shell() -> Result<SetupShell> {
    let current = Shell::detect()?;
    let (path_buf, installed) = preexec_installed(&current);
    let path = super::profile::path_for_display(&path_buf);
    match bootstrap_decision(installed, any_profile_wrapped(), ci_job()) {
        BootstrapDecision::Current => Ok(SetupShell::Current { path }),
        BootstrapDecision::Missing => Ok(SetupShell::Missing),
        BootstrapDecision::SkipCi => Ok(SetupShell::SkippedCi),
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
    fn ci_skips_shell_wrap() {
        assert_eq!(
            bootstrap_decision(false, false, true),
            BootstrapDecision::SkipCi
        );
        assert_eq!(
            bootstrap_decision(true, true, true),
            BootstrapDecision::SkipCi
        );
    }

    #[test]
    fn first_profile_writes_later_warns() {
        assert_eq!(
            bootstrap_decision(false, false, false),
            BootstrapDecision::Write
        );
        assert_eq!(
            bootstrap_decision(true, false, false),
            BootstrapDecision::Current
        );
        assert_eq!(
            bootstrap_decision(false, true, false),
            BootstrapDecision::Missing
        );
    }

    #[test]
    fn ci_env_truthy_values() {
        temp_env::with_var("CI", Some("true"), || assert!(ci_job()));
        temp_env::with_var("CI", Some("1"), || assert!(ci_job()));
        temp_env::with_var("CI", Some("false"), || assert!(!ci_job()));
        temp_env::with_var("CI", Some("0"), || assert!(!ci_job()));
        temp_env::with_var("CI", None::<&str>, || assert!(!ci_job()));
    }
}
