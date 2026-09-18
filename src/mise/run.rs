use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result};

pub async fn run_in_repo(repo: &Path, bin: &Path, args: &[String]) -> Result<std::process::Output> {
    tokio::process::Command::new(bin)
        .args(args)
        .current_dir(repo)
        .stdin(Stdio::null())
        .output()
        .await
        .with_context(|| format!("could not run {} {}", bin.display(), args.join(" ")))
}
