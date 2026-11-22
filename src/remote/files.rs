//! `lade __remote-files <path>`: what a remote sends the laptop at connect,
//! the `lade.yaml` files on the walk of that path, as bytes.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct RemoteFiles {
    pub protocol: u32,
    pub lade: String,
    pub host: String,
    pub user: Option<String>,
    /// As passed to `lade connect`.
    pub path: String,
    pub dir: String,
    pub files: Vec<WalkFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct WalkFile {
    pub path: String,
    pub text: String,
}

impl RemoteFiles {
    /// The walk hash the remote computes for a command in `dir`, with its
    /// saved user.
    pub(crate) fn walk(&self) -> [u8; 32] {
        let files = self
            .files
            .iter()
            .map(|file| (file.path.clone(), file.text.as_bytes().to_vec()))
            .collect::<Vec<_>>();
        super::link_walk(crate::config::hash_walk(&files), &self.user)
    }
}

/// `~/x` and a relative path start at the remote home: the local shell
/// would expand a bare `~` to the laptop's.
fn remote_dir(path: &str) -> Result<PathBuf> {
    let home = crate::home_dir().context("the remote has no home directory")?;
    let joined = match path.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None if path == "~" => home,
        None => home.join(path),
    };
    std::fs::canonicalize(&joined)
        .with_context(|| format!("{path}: {} is not a directory", joined.display()))
}

pub(crate) async fn collect(path: &str) -> Result<RemoteFiles> {
    let dir = remote_dir(path)?;
    let files = crate::config::yaml_files_on_walk(&dir)?
        .into_iter()
        .map(|path| read(&path))
        .collect::<Result<Vec<_>>>()?;
    if files.is_empty() {
        bail!("no lade.yaml on the walk of {}", dir.display());
    }
    Ok(RemoteFiles {
        protocol: lade_sdk::upstream::PROTOCOL,
        lade: env!("CARGO_PKG_VERSION").to_string(),
        host: super::hostname(),
        user: crate::config::saved_user().await?,
        path: path.to_string(),
        dir: dir.to_string_lossy().into_owned(),
        files,
    })
}

fn read(path: &Path) -> Result<WalkFile> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read {} as UTF-8", path.display()))?;
    Ok(WalkFile {
        path: path.to_string_lossy().into_owned(),
        text,
    })
}

pub(crate) async fn run(path: &str) -> Result<()> {
    let files = collect(path).await?;
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(lade_sdk::upstream::MAGIC)?;
    serde_json::to_writer(&mut stdout, &files)?;
    stdout.write_all(b"\n")?;
    stdout.flush()?;
    Ok(())
}

/// The answer after any shell rc output.
pub(crate) fn parse(stdout: &[u8]) -> Result<RemoteFiles> {
    let magic = lade_sdk::upstream::MAGIC;
    let at = stdout
        .windows(magic.len())
        .position(|window| window == magic)
        .context("the remote did not answer as Lade: is lade on its non-interactive PATH?")?;
    let body = &stdout[at + magic.len()..];
    serde_json::from_slice(body).context("the remote's answer is not Lade's")
}
