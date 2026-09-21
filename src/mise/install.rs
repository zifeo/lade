use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

use tokio::io::{AsyncBufReadExt, BufReader};

use super::error::Error;
use super::project;
use super::spec::Spec;

pub async fn install_from_url(spec: &Spec, installs: &Path, cwd: &Path) -> Result<(), Error> {
    let root = crate::cache::prepare_mise_project(cwd, &project::pin_only_toml(spec), None)
        .map_err(|e| Error::install(e.to_string()))?;
    let config = root.join("mise.toml");
    let ignored = project::isolate_config_paths(cwd);
    let args = vec!["install".to_string(), spec.install_arg()];
    let output = run_mise_progress(installs, &root, args.clone(), Some(config), &ignored).await?;
    require_ok(output, &args)
}

pub async fn install_locked(
    lock_src: &Path,
    spec: &Spec,
    installs: &Path,
    cwd: &Path,
) -> Result<(), Error> {
    let root =
        crate::cache::prepare_mise_project(cwd, &project::pin_only_toml(spec), Some(lock_src))
            .map_err(|e| Error::install(e.to_string()))?;
    let config = root.join("mise.toml");
    let args = vec![
        "install".to_string(),
        "--locked".to_string(),
        spec.backend_id(),
    ];
    let ignored = project::isolate_config_paths(cwd);
    let output = run_mise_progress(installs, &root, args.clone(), Some(config), &ignored).await?;
    require_ok(output, &args)
}

pub async fn refresh_lock(
    toml: &str,
    dest: &Path,
    installs: &Path,
    cwd: &Path,
) -> Result<(), Error> {
    let root = crate::cache::prepare_mise_project(cwd, toml, Some(dest))
        .map_err(|e| Error::install(e.to_string()))?;
    let lock = root.join("mise.lock");
    let args = vec!["lock".to_string(), "--upgrade".to_string()];
    let ignored = project::isolate_config_paths(cwd);
    let output = run_mise_progress(installs, &root, args.clone(), None, &ignored).await?;
    require_ok(output, &args)?;
    if !lock.exists() {
        return Err(Error::install(
            "mise lock did not write a lockfile. The command was not started.".to_string(),
        ));
    }
    crate::cache::publish_lock(&lock, dest).map_err(|e| Error::install(e.to_string()))?;
    Ok(())
}

/// Capture only. `mise env --json-extended` must not hit the terminal.
pub(super) async fn run_mise(
    installs: &Path,
    cd: &Path,
    args: Vec<String>,
    config: Option<PathBuf>,
    ignored: &[PathBuf],
) -> Result<Output, Error> {
    run_mise_cmd(installs, cd, args, config, ignored, false).await
}

async fn run_mise_progress(
    installs: &Path,
    cd: &Path,
    args: Vec<String>,
    config: Option<PathBuf>,
    ignored: &[PathBuf],
) -> Result<Output, Error> {
    run_mise_cmd(
        installs,
        cd,
        args,
        config,
        ignored,
        crate::live_progress::is_active(),
    )
    .await
}

async fn run_mise_cmd(
    installs: &Path,
    cd: &Path,
    args: Vec<String>,
    config: Option<PathBuf>,
    ignored: &[PathBuf],
    live: bool,
) -> Result<Output, Error> {
    let shims = crate::cache::mise_shims();
    std::fs::create_dir_all(&shims).map_err(|e| Error::install(e.to_string()))?;
    let mut cmd = tokio::process::Command::new(super::ensure::mise_program());
    cmd.arg("--cd")
        .arg(cd)
        .args(&args)
        .env("MISE_INSTALLS_DIR", installs)
        .env("MISE_SHIMS_DIR", &shims)
        .env("MISE_TRUSTED_CONFIG_PATHS", cd)
        .env("MISE_YES", "1")
        .env("MISE_NOT_FOUND_AUTO_INSTALL", "false")
        .env_remove("MISE_ENV")
        .env_remove("MISE_GLOBAL_CONFIG_FILE")
        .env_remove("MISE_CONFIG_DIR")
        .env_remove("MISE_IGNORED_CONFIG_PATHS")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .current_dir(cd);
    if live {
        cmd.env_remove("MISE_QUIET");
    } else {
        cmd.env("MISE_QUIET", "1");
    }
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
    let mut child = cmd
        .spawn()
        .map_err(|e| Error::missing_mise(e.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| Error::missing_mise("mise stdout was not piped".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| Error::missing_mise("mise stderr was not piped".to_string()))?;
    let (stdout, stderr) = tokio::join!(collect_stream(stdout, live), collect_stream(stderr, live));
    let status = child
        .wait()
        .await
        .map_err(|e| Error::missing_mise(e.to_string()))?;
    Ok(Output {
        status,
        stdout,
        stderr,
    })
}

async fn collect_stream(reader: impl tokio::io::AsyncRead + Unpin, live: bool) -> Vec<u8> {
    let mut lines = BufReader::new(reader).lines();
    let mut buf = Vec::new();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                buf.extend_from_slice(line.as_bytes());
                buf.push(b'\n');
                if live && let Some(shown) = mise_log_line(&line) {
                    crate::live_progress::note(&shown);
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    buf
}

fn mise_log_line(raw: &str) -> Option<String> {
    let line = raw.rsplit('\r').next().unwrap_or(raw).trim_end();
    if line.is_empty() {
        return None;
    }
    Some(line.to_string())
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

#[cfg(test)]
mod tests {
    use super::mise_log_line;

    #[test]
    fn mise_log_line_skips_blank() {
        assert_eq!(mise_log_line(""), None);
        assert_eq!(mise_log_line("   "), None);
        assert_eq!(mise_log_line("\r"), None);
    }

    #[test]
    fn mise_log_line_keeps_last_carriage_state() {
        assert_eq!(
            mise_log_line("mise rust@1.86.0          download").as_deref(),
            Some("mise rust@1.86.0          download")
        );
        assert_eq!(
            mise_log_line("old\rmise rust@1.86.0          install").as_deref(),
            Some("mise rust@1.86.0          install")
        );
    }
}
