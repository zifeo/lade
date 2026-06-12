use std::path::Path;

use anyhow::Result;

use crate::args::StatusCommand;
use crate::compat::{self, all_supported_schemes, known_schemes};
use crate::config::LadeFile;
use crate::global_config::GlobalConfig;
use crate::shell::{self, hook_installed};
use crate::upgrade;

pub async fn run(opts: StatusCommand) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let mut issues = false;

    let version = match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        upgrade::fetch_version_status(),
    )
    .await
    {
        Ok(Ok(status)) => status,
        Ok(Err(e)) => {
            issues = true;
            println!(
                "lade version: {} (update check failed: {e})",
                env!("CARGO_PKG_VERSION")
            );
            return finish(issues);
        }
        Err(_) => {
            issues = true;
            println!(
                "lade version: {} (update check timed out)",
                env!("CARGO_PKG_VERSION")
            );
            return finish(issues);
        }
    };

    println!("lade version: {}", version.current);
    if let Some(latest) = &version.latest {
        if version.update_available {
            issues = true;
            println!("  latest: {latest} (update available — run `lade upgrade`)");
        } else {
            println!("  latest: {latest} (up to date)");
        }
    } else {
        println!("  latest: (not checked recently)");
    }

    let global = GlobalConfig::load().await?;
    let config_path = GlobalConfig::path();
    println!("global config: {}", display_path(&config_path));
    match &global.user {
        Some(user) => println!("  user: {user}"),
        None => println!("  user: (OS default)"),
    }

    let shell = shell::Shell::detect()?;
    let (profile, installed) = hook_installed(&shell);
    println!("shell hooks ({})", shell.bin());
    println!("  profile: {}", display_path(&profile));
    if installed {
        println!("  installed: yes");
    } else {
        issues = true;
        println!("  installed: no (run `lade install`)");
    }

    match LadeFile::build(cwd.clone()) {
        Ok(config) => {
            println!("project config: ok ({} rules)", config.rule_count());
            let saved_user = global.user.or_else(|| {
                std::env::var("USER")
                    .ok()
                    .or_else(|| std::env::var("USERNAME").ok())
            });
            let schemes = if opts.all {
                all_supported_schemes()
            } else {
                let mut schemes = known_schemes(
                    config
                        .all_secret_sources(&saved_user)
                        .iter()
                        .map(|s| s.as_str()),
                );
                schemes.sort();
                schemes
            };
            if schemes.is_empty() {
                println!("vault CLIs: (none referenced in lade.yml)");
            } else {
                println!("vault CLIs:");
                let warnings = compat::check_schemes(schemes).await?;
                if warnings.is_empty() {
                    println!("  all checked CLIs meet minimum versions");
                } else {
                    issues = true;
                    for w in &warnings {
                        println!("  {} {} < {} ({})", w.name, w.found, w.min, w.install_url);
                    }
                }
            }
        }
        Err(e) => {
            issues = true;
            println!("project config: error");
            println!("  {e}");
        }
    }

    finish(issues)
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

fn finish(issues: bool) -> Result<()> {
    if issues {
        std::process::exit(1);
    }
    Ok(())
}
