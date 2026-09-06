use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::args::StatusCommand;
use crate::event;

mod gather;
mod print;
#[cfg(test)]
mod tests;

#[derive(Serialize)]
struct VersionInfo {
    current: String,
    latest: Option<String>,
    update_available: bool,
    check_error: Option<String>,
    last_check: Option<DateTime<Utc>>,
}

#[derive(Serialize)]
struct GlobalConfigInfo {
    path: PathBuf,
    user: Option<String>,
}

#[derive(Serialize)]
struct PreexecHooks {
    shell: String,
    profile: PathBuf,
    installed: bool,
    inject_skips_startup_files: bool,
    /// Set when inject will skip a file or `$BASH_ENV` that exists now.
    inject_startup_skipped: Option<String>,
}

#[derive(Serialize)]
struct HooksInfo {
    preexec: PreexecHooks,
    pretool: crate::pretool::install::PretoolStatus,
}

#[derive(Serialize)]
struct CliWarning {
    name: String,
    found: String,
    min: String,
    install_url: String,
}

#[derive(Serialize)]
struct VaultClis {
    checked: Vec<String>,
    warnings: Vec<CliWarning>,
}

#[derive(Serialize)]
struct ProjectConfig {
    rule_count: usize,
    error: Option<String>,
    vault_clis: VaultClis,
}

#[derive(Serialize)]
struct StatusReport {
    version: VersionInfo,
    global_config: GlobalConfigInfo,
    hooks: HooksInfo,
    skills: crate::pretool::install::SkillsStatus,
    project_config: ProjectConfig,
    log: event::LogInfo,
    ok: bool,
}

pub async fn run(opts: StatusCommand) -> Result<()> {
    let report = gather::gather(&opts).await?;
    if opts.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print::print_human(&report);
    }
    if report.ok {
        return Ok(());
    }
    std::process::exit(crate::exit_codes::FAILURE);
}
