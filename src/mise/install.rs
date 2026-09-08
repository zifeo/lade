use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

use super::error::Error;
use super::project;
use super::spec::Spec;

pub async fn install_from_url(spec: &Spec, installs: &Path, cwd: &Path) -> Result<(), Error> {
    let tmp = tempfile::tempdir().map_err(|e| Error::install(e.to_string()))?;
    let config = tmp.path().join("mise.toml");
    std::fs::write(&config, project::pin_only_toml(spec))
        .map_err(|e| Error::install(e.to_string()))?;
    let ignored = project::isolate_config_paths(cwd);
    let args = vec!["install".to_string(), spec.cli_spec()];
    let output = run_mise(installs, tmp.path(), args.clone(), Some(config), &ignored).await?;
    require_ok(output, &args)
}

pub async fn install_locked(
    tool: &str,
    lock_src: &Path,
    spec: &Spec,
    installs: &Path,
    cwd: &Path,
) -> Result<(), Error> {
    let tmp = tempfile::tempdir().map_err(|e| Error::install(e.to_string()))?;
    let lock_dest = tmp.path().join("mise.lock");
    std::fs::copy(lock_src, &lock_dest).map_err(|e| Error::install(e.to_string()))?;
    std::fs::write(tmp.path().join("mise.toml"), project::pin_only_toml(spec))
        .map_err(|e| Error::install(e.to_string()))?;
    let args = vec![
        "install".to_string(),
        "--locked".to_string(),
        tool.to_string(),
    ];
    let ignored = project::isolate_config_paths(cwd);
    let output = run_mise(
        installs,
        tmp.path(),
        args.clone(),
        Some(tmp.path().join("mise.toml")),
        &ignored,
    )
    .await?;
    require_ok(output, &args)
}

pub(super) async fn run_mise(
    installs: &Path,
    cd: &Path,
    args: Vec<String>,
    config: Option<PathBuf>,
    ignored: &[PathBuf],
) -> Result<Output, Error> {
    let mut cmd = tokio::process::Command::new("mise");
    cmd.arg("--cd")
        .arg(cd)
        .args(&args)
        .env("MISE_INSTALLS_DIR", installs)
        .env("MISE_TRUSTED_CONFIG_PATHS", cd)
        .env("MISE_YES", "1")
        .env("MISE_QUIET", "1")
        .env("MISE_NOT_FOUND_AUTO_INSTALL", "false")
        .env_remove("MISE_ENV")
        .env_remove("MISE_GLOBAL_CONFIG_FILE")
        .env_remove("MISE_CONFIG_DIR")
        .env_remove("MISE_IGNORED_CONFIG_PATHS")
        .stdin(Stdio::null())
        .current_dir(cd);
    if let Some(config) = config {
        cmd.env("MISE_GLOBAL_CONFIG_FILE", &config);
        if let Some(dir) = config.parent() {
            cmd.env("MISE_CONFIG_DIR", dir);
        }
    }
    if !ignored.is_empty() {
        let joined = std::env::join_paths(ignored).unwrap_or_default();
        cmd.env("MISE_IGNORED_CONFIG_PATHS", joined);
    }
    cmd.output()
        .await
        .map_err(|e| Error::missing_mise(e.to_string()))
}

pub(super) fn failure_detail(output: &Output, args: &[String]) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        format!("mise {} failed", args.join(" "))
    }
}

fn require_ok(output: Output, args: &[String]) -> Result<(), Error> {
    if output.status.success() {
        return Ok(());
    }
    Err(Error::install(failure_detail(&output, args)))
}
