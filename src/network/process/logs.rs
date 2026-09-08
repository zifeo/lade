use anyhow::{Result, bail};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

static LOG_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) struct ChildOutputFiles {
    stdout_path: PathBuf,
    stderr_path: PathBuf,
}

impl ChildOutputFiles {
    pub(crate) fn capture(command: &mut Command) -> Result<Self> {
        let (stdout_path, stdout) = create_log_file("stdout")?;
        let (stderr_path, stderr) = create_log_file("stderr")?;
        command
            .stdout(Stdio::from(stdout))
            .stderr(Stdio::from(stderr));
        Ok(Self {
            stdout_path,
            stderr_path,
        })
    }

    pub(crate) fn read_text(&self) -> String {
        let stdout = fs::read_to_string(&self.stdout_path).unwrap_or_default();
        let stderr = fs::read_to_string(&self.stderr_path).unwrap_or_default();
        let mut parts = Vec::new();
        if !stdout.trim().is_empty() {
            parts.push(format!("stdout:\n{}", dedupe_lines(&stdout)));
        }
        if !stderr.trim().is_empty() {
            parts.push(format!("stderr:\n{}", dedupe_lines(&stderr)));
        }
        parts.join("\n")
    }

    pub(crate) fn cleanup(&self) {
        let _ = fs::remove_file(&self.stdout_path);
        let _ = fs::remove_file(&self.stderr_path);
    }
}

fn create_log_file(stream: &str) -> Result<(PathBuf, File)> {
    // macOS TMPDIR is under /var/folders and is not created until first use.
    let dir = std::env::temp_dir();
    fs::create_dir_all(&dir)?;
    for _ in 0..16 {
        let idx = LOG_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!(
            "lade-network-{}-{idx}-{stream}.log",
            std::process::id()
        ));
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    bail!("could not create network provider log file")
}

pub(super) fn dedupe_lines(raw: &str) -> String {
    let mut seen = HashSet::new();
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim_end();
            if trimmed.is_empty() || !seen.insert(dedupe_key(trimmed)) {
                None
            } else {
                Some(trimmed)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn dedupe_key(line: &str) -> String {
    if let Some((_, rest)) = line.split_once(" memcache.go:") {
        return format!("memcache.go:{rest}");
    }
    line.to_string()
}
