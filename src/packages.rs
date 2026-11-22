use std::path::Path;

use anyhow::{Context, Result};

use crate::config::LadeFile;
use crate::message_box::Report;
use crate::mise;

pub async fn run(verb: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    // § vwj3wvrzs.d@w6h11
    let Some(git) = crate::catalog::git_root(&cwd) else {
        return Ok(());
    };
    let config = LadeFile::build(cwd.clone())?;
    let saved = crate::global_config::GlobalConfig::resolved_user()?;
    let mut ran = Vec::new();
    for (key, uri) in config.package_uris(&saved) {
        // § vwj3wvrzs.b@wbbmd
        let Some((cli, reference)) = parse_package(&uri) else {
            continue;
        };
        let args = match verb {
            "setup" => install_args(cli, reference),
            "teardown" => teardown_args(cli, reference),
            _ => continue,
        };
        let label = match verb {
            "teardown" => format!("Removing {cli} {reference}."),
            _ => format!("Installing {cli} {reference}."),
        };
        if crate::live_progress::is_active() {
            crate::live_progress::running(format!("pkg-{key}"), &label);
            run_package_cli(&git, cli, &args).await?;
            crate::live_progress::done(format!("pkg-{key}"), &label);
        } else {
            Report::progress(label);
            run_package_cli(&git, cli, &args).await?;
        }
        ran.push(format!("{key} {cli} {reference}"));
    }
    if crate::live_progress::is_active() {
        return Ok(());
    }
    if !ran.is_empty() {
        let mut report = Report::new().heading(format!("{verb} setup packages"));
        for line in ran {
            report = report.line(format!("  {line}"));
        }
        report.print();
    }
    Ok(())
}

pub fn parse_package(uri: &str) -> Option<(&'static str, &str)> {
    if let Some(rest) = uri.strip_prefix("apm://") {
        return Some(("apm", rest.split(['?', '#']).next().unwrap_or(rest)));
    }
    if let Some(rest) = uri.strip_prefix("skills://") {
        return Some(("skills", rest.split(['?', '#']).next().unwrap_or(rest)));
    }
    None
}

fn install_args(cli: &str, reference: &str) -> Vec<String> {
    match cli {
        "apm" => vec!["install".to_string(), reference.to_string()],
        "skills" => vec!["add".to_string(), reference.to_string(), "-y".to_string()],
        _ => Vec::new(),
    }
}

fn teardown_args(cli: &str, reference: &str) -> Vec<String> {
    match cli {
        "apm" => vec!["uninstall".to_string(), reference.to_string()],
        "skills" => vec![
            "remove".to_string(),
            reference.to_string(),
            "-y".to_string(),
        ],
        _ => Vec::new(),
    }
}

async fn run_package_cli(repo: &Path, cli: &str, args: &[String]) -> Result<()> {
    // § vwj3wvrzs.e@v9tfb
    if args.is_empty() {
        return Ok(());
    }
    let bin = mise::locked_cli_bin(cli)
        .with_context(|| format!("locked {cli} is missing. run `lade setup`"))?;
    let output = mise::run_in_repo(repo, &bin, args).await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{cli} {} failed: {stderr}", args.join(" "));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_apm_and_skills() {
        assert_eq!(
            parse_package("apm://github/destructure-command-hook"),
            Some(("apm", "github/destructure-command-hook"))
        );
        assert_eq!(
            parse_package("skills://vercel-labs/agent-skills"),
            Some(("skills", "vercel-labs/agent-skills"))
        );
        assert!(parse_package("mise://aqua/jqlang/jq@1.7.1").is_none());
    }

    /// Runs `verb` in a repo whose yaml binds `uri` on `.`, with a stub of
    /// the locked `cli` that records its arguments and exits `exit`.
    #[cfg(unix)]
    fn run_with_stub(
        verb: &str,
        cli: &str,
        version: &str,
        uri: &str,
        exit: i32,
        git: bool,
    ) -> (Result<()>, String) {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let installs = dir.path().join("installs");
        let bin_dir = installs.join(cli).join(version);
        std::fs::create_dir_all(&bin_dir).unwrap();
        let bin = bin_dir.join(cli);
        std::fs::write(
            &bin,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$*\" >> \"$MISE_INSTALLS_DIR/{cli}-args\"\nexit {exit}\n"
            ),
        )
        .unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        if git {
            std::fs::create_dir_all(dir.path().join(".git")).unwrap();
        }
        std::fs::write(dir.path().join("lade.yaml"), format!(".:\n  pkg: {uri}\n")).unwrap();
        let result = temp_env::with_vars(
            [("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap()))],
            || {
                let prev = std::env::current_dir().unwrap();
                std::env::set_current_dir(dir.path()).unwrap();
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(run(verb));
                std::env::set_current_dir(prev).unwrap();
                result
            },
        );
        let args =
            std::fs::read_to_string(installs.join(format!("{cli}-args"))).unwrap_or_default();
        (result, args)
    }

    #[cfg(unix)]
    #[test]
    fn setup_runs_locked_apm() {
        let (result, args) = run_with_stub(
            "setup",
            "apm",
            "0.23.1",
            "apm://github/destructure-command-hook",
            0,
            true,
        );
        result.unwrap();
        assert!(
            args.contains("install github/destructure-command-hook"),
            "{args}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn teardown_runs_locked_apm_uninstall() {
        let (result, args) = run_with_stub(
            "teardown",
            "apm",
            "0.23.1",
            "apm://github/destructure-command-hook",
            0,
            true,
        );
        result.unwrap();
        assert_eq!(args.trim(), "uninstall github/destructure-command-hook");
    }

    #[cfg(unix)]
    #[test]
    fn skills_setup_adds_with_yes() {
        let (result, args) = run_with_stub(
            "setup",
            "skills",
            "1.4.0",
            "skills://vercel-labs/agent-skills",
            0,
            true,
        );
        result.unwrap();
        assert_eq!(args.trim(), "add vercel-labs/agent-skills -y");
    }

    #[cfg(unix)]
    #[test]
    fn run_outside_git_does_nothing() {
        let (result, args) = run_with_stub(
            "setup",
            "apm",
            "0.23.1",
            "apm://github/destructure-command-hook",
            0,
            false,
        );
        result.unwrap();
        assert!(args.is_empty(), "{args}");
    }

    #[cfg(unix)]
    #[test]
    fn failing_package_cli_fails_the_verb() {
        let (result, _) = run_with_stub(
            "setup",
            "apm",
            "0.23.1",
            "apm://github/destructure-command-hook",
            2,
            true,
        );
        let err = result.unwrap_err().to_string();
        assert!(err.contains("apm install"), "{err}");
    }
}
