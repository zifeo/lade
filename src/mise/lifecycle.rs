use std::path::Path;

use anyhow::{Context, Result};

use crate::config::LadeFile;
use crate::message_box::Report;

use super::spec;

pub async fn run_lifecycle_commands(verb: &str) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let Some(git) = crate::catalog::git_root(&cwd) else {
        return Ok(());
    };
    let config = LadeFile::build(cwd.clone())?;
    let saved = crate::global_config::GlobalConfig::user_from_disk();
    let mut ran = Vec::new();
    for (key, value) in config.pins(&saved) {
        let Ok(spec) = spec::parse(&value) else {
            continue;
        };
        let Some(command) = spec.options.get(verb) else {
            continue;
        };
        if command.is_empty() {
            continue;
        }
        let label = format!("{key} {command}");
        if crate::live_progress::is_active() {
            crate::live_progress::running(format!("cmd-{key}"), &label);
            run_bin_command(&git, &key, &spec, command).await?;
            crate::live_progress::done(format!("cmd-{key}"), &label);
        } else {
            Report::progress(format!("Running {key} {command}."));
            run_bin_command(&git, &key, &spec, command).await?;
        }
        ran.push(format!("{key} {command}"));
    }
    if crate::live_progress::is_active() {
        return Ok(());
    }
    if !ran.is_empty() {
        let mut report = Report::new().heading(format!("{verb} commands"));
        for line in ran {
            report = report.line(format!("  {line}"));
        }
        report.print();
    }
    Ok(())
}

async fn run_bin_command(repo: &Path, key: &str, spec: &spec::Spec, command: &str) -> Result<()> {
    let bin = super::locked_bin(Some(repo), key, spec)
        .with_context(|| format!("locked {key} is missing. run `lade setup`"))?;
    let args: Vec<String> = command.split_whitespace().map(str::to_string).collect();
    let output = super::run_in_repo(repo, &bin, &args).await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("{key} {command} failed: {stderr}");
    }
    Ok(())
}
