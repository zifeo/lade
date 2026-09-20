use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::message_box::MessageBox;

const FETCH_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
}

pub fn release_asset(tag: &str) -> Result<String, String> {
    let os = match std::env::consts::OS {
        "macos" => "macos",
        "linux" => "linux",
        other => {
            return Err(format!(
                "No official mise build for OS `{other}`. Lade can fetch mise on macOS and Linux. Install mise for this OS, put it on PATH, then re-run `lade setup`. https://mise.jdx.dev/installing-mise.html"
            ));
        }
    };
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => {
            return Err(format!(
                "No official mise build for arch `{other}`. Lade can fetch mise for aarch64 and x86_64. Install mise for this CPU, put it on PATH, then re-run `lade setup`. https://mise.jdx.dev/installing-mise.html"
            ));
        }
    };
    Ok(format!("mise-{tag}-{os}-{arch}.tar.gz"))
}

pub async fn fetch_official(dest: PathBuf) -> Result<(), String> {
    if std::env::var_os("LADE_MISE_FETCH")
        .as_deref()
        .is_some_and(|v| v == "0")
    {
        return Err(
            "mise fetch is off (`LADE_MISE_FETCH=0`). Install mise yourself and put it on PATH, or unset that variable and re-run `lade setup`. https://mise.jdx.dev/installing-mise.html"
                .to_string(),
        );
    }
    MessageBox::new()
        .info()
        .line("Fetching the official mise release.")
        .print_stderr();
    tokio::task::spawn_blocking(move || download_and_install(&dest))
        .await
        .map_err(|e| e.to_string())?
}

fn download_and_install(dest: &Path) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .user_agent(format!("lade/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let release: GithubRelease = client
        .get("https://api.github.com/repos/jdx/mise/releases/latest")
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .map_err(|e| e.to_string())?;
    let tag = release.tag_name;
    let asset = release_asset(&tag)?;
    let url = format!("https://github.com/jdx/mise/releases/download/{tag}/{asset}");
    let bytes = client
        .get(&url)
        .send()
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .bytes()
        .map_err(|e| e.to_string())?;
    unpack_mise(&bytes, dest)
}

fn unpack_mise(bytes: &[u8], dest: &Path) -> Result<(), String> {
    let decoder = flate2::read::GzDecoder::new(Cursor::new(bytes));
    let mut archive = tar::Archive::new(decoder);
    let parent = dest
        .parent()
        .ok_or_else(|| "mise install path is missing a parent".to_string())?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    for entry in archive.entries().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path().map_err(|e| e.to_string())?;
        if path.file_name().and_then(|n| n.to_str()) != Some("mise") {
            continue;
        }
        if !entry.header().entry_type().is_file() {
            continue;
        }
        let tmp = parent.join(".mise.download");
        entry.unpack(&tmp).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, dest).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    Err("the mise archive did not contain a mise binary".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_name_is_os_arch() {
        let name = release_asset("v2025.8.12").unwrap();
        assert!(name.starts_with("mise-v2025.8.12-"), "{name}");
        assert!(name.ends_with(".tar.gz"), "{name}");
    }
}
