use std::path::PathBuf;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::args::StatusCommand;
use crate::event;

mod format;
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
struct AgePluginInfo {
    present: bool,
    path: Option<PathBuf>,
}

#[derive(Serialize)]
struct GlobalConfigInfo {
    path: PathBuf,
    user: Option<String>,
    /// Raw from `config.json`. `max_cache_ms` 0: the hub keeps nothing.
    max_cache: Option<serde_json::Value>,
    max_cache_ms: Option<u32>,
    /// Set when the value is invalid: the hub then keeps nothing.
    max_cache_error: Option<String>,
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
    /// `None` when no shell started Lade.
    preexec: Option<PreexecHooks>,
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
struct ProviderInfo {
    scheme: String,
    name: String,
    transport: String,
    batch_unit: String,
    version: Option<String>,
    timeout_s: u64,
}

#[derive(Serialize)]
struct ProjectConfig {
    rule_count: usize,
    error: Option<String>,
    vault_clis: VaultClis,
    providers: Vec<ProviderInfo>,
}

#[derive(Serialize)]
struct LockedTool {
    name: String,
    version: Option<String>,
    present: bool,
}

#[derive(Serialize)]
struct MiseInfo {
    needed: bool,
    version: Option<String>,
    in_range: bool,
    range: String,
    tools: Vec<LockedTool>,
}

/// Names and ttl only. The hub never hands out values here.
#[derive(Serialize)]
struct HubKey {
    name: String,
    cwd: String,
    ttl_left_ms: u32,
}

#[derive(Serialize)]
struct HubTunnel {
    key: String,
    uri: String,
    port: u16,
    leases: u32,
    ttl_left_ms: Option<u32>,
    phase: &'static str,
    #[serde(skip)]
    state: String,
}

#[derive(Serialize)]
struct HubStatus {
    #[serde(flatten)]
    info: crate::hub::HubInfo,
    /// This cwd's `lade ttl set` window.
    window: Option<String>,
    /// Cached secret names valid for this cwd.
    keys: Vec<HubKey>,
    /// Cached secret names on this hub, every cwd.
    key_count: usize,
    tunnels: Vec<HubTunnel>,
    /// The `max_cache` the hub enforces. `lade status` also reads the file:
    /// the two differ until the hub's watcher catches up.
    max_cache_ms: Option<u32>,
    /// Whether another process of this user can read the hub's memory.
    memory: crate::hub::MemoryGuard,
}

#[derive(Serialize)]
struct StatusReport {
    version: VersionInfo,
    age_plugin: AgePluginInfo,
    global_config: GlobalConfigInfo,
    hooks: HooksInfo,
    project_config: ProjectConfig,
    log: event::LogInfo,
    hub: HubStatus,
    mise: MiseInfo,
    ok: bool,
    /// Text only. The JSON keeps the per-location `installed` / `current`.
    #[serde(skip)]
    hook_notices: Vec<String>,
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
