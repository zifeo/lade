use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;

use crate::args::StatusCommand;
use crate::compat::{self, all_supported_schemes, known_schemes};
use crate::config::LadeFile;
use crate::global_config::GlobalConfig;
use crate::shell::{self, hook_installed};
use crate::upgrade;

#[derive(Serialize)]
struct VersionInfo {
    current: String,
    latest: Option<String>,
    update_available: bool,
    check_error: Option<String>,
}

#[derive(Serialize)]
struct GlobalConfigInfo {
    path: PathBuf,
    user: Option<String>,
}

#[derive(Serialize)]
struct HooksInfo {
    shell: String,
    profile: PathBuf,
    installed: bool,
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
    project_config: ProjectConfig,
    ok: bool,
}

pub async fn run(opts: StatusCommand) -> Result<()> {
    let report = gather(&opts).await?;
    if opts.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human(&report);
    }
    if report.ok {
        return Ok(());
    }
    std::process::exit(crate::exit_codes::FAILURE);
}

async fn gather(opts: &StatusCommand) -> Result<StatusReport> {
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
        },
        Ok(Err(e)) => VersionInfo {
            current,
            latest: None,
            update_available: false,
            check_error: Some(e.to_string()),
        },
        Err(_) => VersionInfo {
            current,
            latest: None,
            update_available: false,
            check_error: Some("update check timed out".to_string()),
        },
    };

    let global = GlobalConfig::load().await?;
    let global_config = GlobalConfigInfo {
        path: GlobalConfig::path(),
        user: global.user.clone(),
    };

    let shell = shell::Shell::detect()?;
    let (profile, installed) = hook_installed(&shell);
    let hooks = HooksInfo {
        shell: shell.bin().to_string(),
        profile,
        installed,
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
        && hooks.installed
        && project_config.error.is_none()
        && project_config.vault_clis.warnings.is_empty();

    Ok(StatusReport {
        version,
        global_config,
        hooks,
        project_config,
        ok,
    })
}

fn print_human(report: &StatusReport) {
    let v = &report.version;
    match &v.check_error {
        Some(err) => println!("lade version: {} ({err})", v.current),
        None => {
            println!("lade version: {}", v.current);
            match (&v.latest, v.update_available) {
                (Some(latest), true) => {
                    println!("  latest: {latest} (update available — run `lade upgrade`)")
                }
                (Some(latest), false) => println!("  latest: {latest} (up to date)"),
                (None, _) => println!("  latest: (not checked recently)"),
            }
        }
    }

    println!(
        "global config: {}",
        display_path(&report.global_config.path)
    );
    match &report.global_config.user {
        Some(user) => println!("  user: {user}"),
        None => println!("  user: (OS default)"),
    }

    println!("shell hooks ({})", report.hooks.shell);
    println!("  profile: {}", display_path(&report.hooks.profile));
    if report.hooks.installed {
        println!("  installed: yes");
    } else {
        println!("  installed: no (run `lade install`)");
    }

    let pc = &report.project_config;
    if let Some(err) = &pc.error {
        println!("project config: error");
        println!("  {err}");
        return;
    }
    println!("project config: ok ({} rules)", pc.rule_count);
    if pc.vault_clis.checked.is_empty() {
        println!("provider CLIs: (none referenced in lade.yml)");
    } else if pc.vault_clis.warnings.is_empty() {
        println!("provider CLIs:");
        println!("  all checked CLIs meet minimum versions");
    } else {
        println!("provider CLIs:");
        for w in &pc.vault_clis.warnings {
            println!("  {} {} < {} ({})", w.name, w.found, w.min, w.install_url);
        }
    }
}

fn display_path(path: &Path) -> String {
    if let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf())
        && let Ok(stripped) = path.strip_prefix(&home)
    {
        if stripped.as_os_str().is_empty() {
            return "~".to_string();
        }
        return format!("~/{}", stripped.display());
    }
    path.display().to_string()
}
