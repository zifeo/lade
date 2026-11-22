use anyhow::Result;

use crate::args::StatusCommand;
use crate::command::upgrade;
use crate::compat::{all_supported_schemes, known_schemes};
use crate::config::LadeFile;
use crate::event;
use crate::global_config::GlobalConfig;
use crate::preexec::{Shell, preexec_installed};
use crate::pretool;
use lade_sdk::Transport;
use lade_sdk::compat::spec_for;

use crate::command::ttl;

use super::{
    AgePluginInfo, GlobalConfigInfo, HooksInfo, HubKey, HubStatus, HubTunnel, LinkStatus,
    PreexecHooks, ProjectConfig, ProviderInfo, RemoteStatus, StatusReport, VaultClis, VersionInfo,
};

pub(super) async fn gather(opts: &StatusCommand) -> Result<StatusReport> {
    let cwd = std::env::current_dir()?;

    let status = upgrade::version_status();
    let version = VersionInfo {
        current: status.current,
        latest: status.latest,
        update_available: status.update_available,
        check_error: status.check_error,
        last_check: status.last_check,
    };

    // A `config.json` the hub cannot read turns its cache off, so status
    // reports it on the max cache row instead of failing before it prints.
    // § cfan04ksd.e@f5mxx
    let (global, max_cache) = match GlobalConfig::load().await {
        Ok(global) => {
            let max_cache = crate::global_config::max_cache_ms(global.max_cache.as_ref());
            (Some(global), max_cache)
        }
        Err(e) => (
            None,
            Err(format!(
                "{} cannot be read: {e:#}",
                GlobalConfig::path().display()
            )),
        ),
    };
    // § end cfan04ksd.e
    let global_config = GlobalConfigInfo {
        path: GlobalConfig::path(),
        user: global.as_ref().and_then(|global| global.user.clone()),
        max_cache: global.as_ref().and_then(|global| global.max_cache.clone()),
        max_cache_ms: max_cache.clone().ok().flatten(),
        max_cache_error: max_cache.err(),
    };

    // The pre-exec hook belongs to the shell that runs `lade status`.
    // § fmn2jmqxb.b@effe9
    let preexec = Shell::detect().ok().map(|shell| {
        let (profile, installed) = preexec_installed(&shell);
        PreexecHooks {
            shell: shell.bin().to_string(),
            profile,
            installed,
            inject_skips_startup_files: true,
            inject_startup_skipped: inject_startup_skipped(&shell),
        }
    });
    // § end fmn2jmqxb.b
    let hooks = HooksInfo {
        preexec,
        pretool: pretool::install::inspect(&cwd)?,
    };

    // A `config.json` that fails is already reported above and fails `ok`, so
    // status lists the schemes of no user rather than stop.
    // § one dcrxp986s.b@zyv6y
    let saved_user = GlobalConfig::resolved_user().unwrap_or(None);

    let project_config = match LadeFile::build(cwd.clone()) {
        Ok(config) => {
            let schemes = if opts.all {
                all_supported_schemes()
            } else {
                let mut schemes = known_schemes(
                    config
                        .all_secret_sources(&saved_user)
                        .into_iter()
                        .chain(config.all_network_sources(&saved_user))
                        .collect::<Vec<_>>()
                        .iter()
                        .map(|s| s.as_str()),
                );
                schemes.sort();
                schemes
            };
            let providers = provider_info(&schemes);
            let cli_checked: Vec<String> = schemes
                .iter()
                .filter(|scheme| spec_for(scheme).is_some())
                .cloned()
                .collect();
            ProjectConfig {
                rule_count: config.rule_count(),
                error: None,
                vault_clis: VaultClis {
                    checked: cli_checked,
                    warnings: Vec::new(),
                },
                providers,
            }
        }
        Err(e) => ProjectConfig {
            rule_count: 0,
            error: Some(format!("{e:#}")),
            vault_clis: VaultClis {
                checked: vec![],
                warnings: vec![],
            },
            providers: vec![],
        },
    };

    let mise_status = crate::mise::status_info().await;
    let saved = saved_user.clone();
    let mut tools = match LadeFile::build(cwd.clone()) {
        Ok(config) => crate::mise::locked_tools(&cwd, &config, &saved)
            .into_iter()
            .map(|tool| super::LockedTool {
                name: tool.name,
                version: tool.version,
                present: tool.present,
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    tools.sort_by(|a, b| a.name.cmp(&b.name));
    let tools_ok = tools.iter().all(|tool| tool.present);
    let mise = super::MiseInfo {
        needed: mise_status.needed,
        version: mise_status.version,
        in_range: mise_status.in_range,
        range: mise_status.range,
        tools,
    };
    // A failed GitHub check stays in the hub ledger until the next one, a
    // day later. It says nothing about this machine's setup.
    // § vk7cw8j73.a@xaarp
    let ok = project_config.error.is_none()
        && (!mise.needed || mise.in_range)
        && tools_ok
        && global_config.max_cache_error.is_none();
    // § end vk7cw8j73.a

    let repo = crate::catalog::git_root(&cwd).map(|p| p.to_string_lossy().into_owned());
    Ok(StatusReport {
        version,
        age_plugin: age_plugin_info(),
        global_config,
        hooks,
        project_config,
        log: event::info_for_repo(repo.as_deref()),
        hub: hub_status(&cwd),
        mise,
        remote: remote_status(),
        ok,
        hook_notices: pretool::install::health_report()?,
    })
}

fn remote_status() -> RemoteStatus {
    // § a36hqp4ep.d@t7bep
    use crate::remote::{MarkerState, read_marker, socket_path};
    let (upstream, laptop, connect) = match read_marker() {
        MarkerState::None => ("none", None, None),
        MarkerState::Unreadable(_) => ("broken", None, None),
        MarkerState::Attached(marker) => {
            let up = std::os::unix::net::UnixStream::connect(socket_path()).is_ok();
            (
                if up { "attached" } else { "broken" },
                Some(marker.laptop),
                Some(crate::remote::connect_line(&marker.transport)),
            )
        }
    };
    let links = crate::hub::link_list()
        .into_iter()
        .map(|row| LinkStatus {
            host: row.host,
            transport: row.key.split('\u{0}').map(str::to_string).collect(),
            phase: match row.phase {
                crate::hub::LinkPhase::Starting => "starting",
                crate::hub::LinkPhase::Ready => "ready",
                crate::hub::LinkPhase::Retrying => "retrying",
                crate::hub::LinkPhase::Failed => "failed",
                crate::hub::LinkPhase::Stopped => "stopped",
            },
            error: row.error,
            walks: row.walks,
        })
        .collect();
    RemoteStatus {
        upstream,
        laptop,
        connect,
        links,
    }
}

/// Never starts a hub: every call only talks to one already up.
fn hub_status(cwd: &std::path::Path) -> HubStatus {
    let info = crate::hub::probe();
    if info.state != crate::hub::HubState::Up {
        return HubStatus {
            info,
            window: None,
            keys: Vec::new(),
            key_count: 0,
            tunnels: Vec::new(),
            max_cache_ms: None,
            memory: crate::hub::memory_guard(),
        };
    }
    let listing = crate::hub::listing();
    let scope = crate::hub::scope(cwd);
    let keys = ttl::rows_for_scope(&listing.rows, Some(&scope), false)
        .into_iter()
        .map(|row| HubKey {
            name: row.name.clone(),
            cwd: row.cwd.clone(),
            ttl_left_ms: row.ttl_left_ms,
        })
        .collect();
    let tunnels = listing
        .tunnels
        .iter()
        .map(|tunnel| HubTunnel {
            key: tunnel.key.clone(),
            uri: tunnel.uri.clone(),
            port: tunnel.port,
            leases: tunnel.leases,
            ttl_left_ms: tunnel.ttl_left_ms,
            phase: ttl::phase_name(tunnel.phase),
            state: ttl::tunnel_state(tunnel),
        })
        .collect();
    // § rbzjhj8tk.b@7zwrr
    let memory = match &info.unguarded {
        Some(detail) => crate::hub::MemoryGuard::Off {
            detail: detail.clone(),
        },
        None => crate::hub::memory_guard(),
    };
    // § end rbzjhj8tk.b
    HubStatus {
        info,
        window: crate::hub::window(&scope),
        keys,
        key_count: listing.rows.len(),
        tunnels,
        max_cache_ms: listing.max_cache_ms,
        memory,
    }
}

const AGE_PLUGIN_BIN: &str = "age-plugin-lade";

fn age_plugin_info() -> AgePluginInfo {
    match find_age_plugin() {
        Some(path) => AgePluginInfo {
            present: true,
            path: Some(path),
        },
        None => AgePluginInfo {
            present: false,
            path: None,
        },
    }
}

fn find_age_plugin() -> Option<std::path::PathBuf> {
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.to_path_buf()));
    find_age_plugin_from(exe_dir.as_deref(), std::env::var_os("PATH").as_deref())
}

fn find_age_plugin_from(
    exe_dir: Option<&std::path::Path>,
    path_var: Option<&std::ffi::OsStr>,
) -> Option<std::path::PathBuf> {
    if let Some(dir) = exe_dir {
        let sibling = dir.join(AGE_PLUGIN_BIN);
        if sibling.is_file() {
            return Some(sibling);
        }
    }
    let path = path_var?;
    for dir in std::env::split_paths(path) {
        let candidate = dir.join(AGE_PLUGIN_BIN);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn provider_info(schemes: &[String]) -> Vec<ProviderInfo> {
    let providers = lade_sdk::Providers::new();
    let mut out: Vec<ProviderInfo> = schemes
        .iter()
        .filter_map(|scheme| {
            let provider = providers.provider(scheme)?;
            let (transport, version) = match provider.transport() {
                Transport::Sdk => ("sdk".to_string(), Some(lade_sdk::VERSION.to_string())),
                Transport::Cli => ("cli".to_string(), None),
            };
            Some(ProviderInfo {
                scheme: scheme.clone(),
                name: provider.name().to_string(),
                transport,
                batch_unit: provider.batch_unit().to_string(),
                version,
                timeout_s: provider.timeout().as_secs(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.scheme.cmp(&b.scheme));
    out
}

fn inject_startup_skipped(shell: &Shell) -> Option<String> {
    if matches!(shell, Shell::Bash) && std::env::var_os("BASH_ENV").is_some() {
        return Some("BASH_ENV".to_string());
    }
    let path = shell.wrap_startup_file()?;
    if !path.is_file() {
        return None;
    }
    Some(
        path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("startup file")
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn sibling_wins_over_path() {
        let exe_dir = tempdir().unwrap();
        let path_dir = tempdir().unwrap();
        let sibling = exe_dir.path().join(AGE_PLUGIN_BIN);
        let on_path = path_dir.path().join(AGE_PLUGIN_BIN);
        fs::write(&sibling, "").unwrap();
        fs::write(&on_path, "").unwrap();
        let path = OsString::from(path_dir.path());
        assert_eq!(
            find_age_plugin_from(Some(exe_dir.path()), Some(path.as_os_str())),
            Some(sibling)
        );
    }

    #[test]
    fn path_is_used_when_no_sibling() {
        let exe_dir = tempdir().unwrap();
        let path_dir = tempdir().unwrap();
        let on_path = path_dir.path().join(AGE_PLUGIN_BIN);
        fs::write(&on_path, "").unwrap();
        let path = OsString::from(path_dir.path());
        assert_eq!(
            find_age_plugin_from(Some(exe_dir.path()), Some(path.as_os_str())),
            Some(on_path)
        );
    }

    #[test]
    fn missing_everywhere_is_none() {
        let exe_dir = tempdir().unwrap();
        let path_dir = tempdir().unwrap();
        let path = OsString::from(path_dir.path());
        assert_eq!(
            find_age_plugin_from(Some(exe_dir.path()), Some(path.as_os_str())),
            None
        );
    }
}
