use anyhow::Result;

use crate::args::StatusCommand;
use crate::compat::{self, all_supported_schemes, known_schemes};
use crate::config::LadeFile;
use crate::event;
use crate::global_config::GlobalConfig;
use crate::pretool;
use crate::shell::{self, preexec_installed};
use crate::upgrade;

use super::{
    CliWarning, GlobalConfigInfo, HooksInfo, PreexecHooks, ProjectConfig, StatusReport, VaultClis,
    VersionInfo,
};

pub(super) async fn gather(opts: &StatusCommand) -> Result<StatusReport> {
    let cwd = std::env::current_dir()?;
    let current = env!("CARGO_PKG_VERSION").to_string();

    let version = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        upgrade::fetch_version_status(),
    )
    .await
    {
        Ok(Ok(status)) => VersionInfo {
            current: status.current,
            latest: status.latest,
            update_available: status.update_available,
            check_error: None,
            last_check: status.last_check,
        },
        Ok(Err(e)) => VersionInfo {
            current,
            latest: None,
            update_available: false,
            check_error: Some(e.to_string()),
            last_check: None,
        },
        Err(_) => VersionInfo {
            current,
            latest: None,
            update_available: false,
            check_error: Some("update check timed out".to_string()),
            last_check: None,
        },
    };

    let global = GlobalConfig::load().await?;
    let global_config = GlobalConfigInfo {
        path: GlobalConfig::path(),
        user: global.user.clone(),
    };

    let shell = shell::Shell::detect()?;
    let (profile, installed) = preexec_installed(&shell);
    let hooks = HooksInfo {
        preexec: PreexecHooks {
            shell: shell.bin().to_string(),
            profile,
            installed,
            inject_skips_startup_files: true,
            inject_startup_skipped: inject_startup_skipped(&shell),
        },
        pretool: pretool::install::inspect(&cwd)?,
    };

    let saved_user = global.user.or_else(|| {
        std::env::var("USER")
            .ok()
            .or_else(|| std::env::var("USERNAME").ok())
    });

    let project_config = match LadeFile::build(cwd) {
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
            let warnings = compat::check_schemes(schemes.clone())
                .await?
                .into_iter()
                .map(|w| CliWarning {
                    name: w.name,
                    found: w.found,
                    min: w.min,
                    install_url: w.install_url,
                })
                .collect();
            ProjectConfig {
                rule_count: config.rule_count(),
                error: None,
                vault_clis: VaultClis {
                    checked: schemes,
                    warnings,
                },
            }
        }
        Err(e) => ProjectConfig {
            rule_count: 0,
            error: Some(e.to_string()),
            vault_clis: VaultClis {
                checked: vec![],
                warnings: vec![],
            },
        },
    };

    let ok = version.check_error.is_none()
        && !version.update_available
        && hooks.preexec.installed
        && project_config.error.is_none()
        && project_config.vault_clis.warnings.is_empty();

    Ok(StatusReport {
        version,
        global_config,
        hooks,
        project_config,
        log: event::info(),
        ok,
    })
}

fn inject_startup_skipped(shell: &shell::Shell) -> Option<String> {
    if matches!(shell, shell::Shell::Bash) && std::env::var_os("BASH_ENV").is_some() {
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
