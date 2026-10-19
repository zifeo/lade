// Go age 1.3 native keys (`age1pq1`, `AGE-SECRET-KEY-PQ-1`). rage 0.12 only
// has tagpq encrypt. str4d/rage#632 (draft, 0.13) adds pq recipient + identity.
// Until it ships, age-plugin-pq (Go age 1.3.2 release) does the crypto.
// An `age1pq1` recipient is already a plugin recipient for it, and
// `age-plugin-pq -identity` turns the native secret into an `AGE-PLUGIN-PQ-1`
// identity the age crate can drive. Remove this file once rage 0.13 lands.

use std::io::{Cursor, Write};
use std::process::{Command, Stdio};

use age::plugin::{RecipientPluginV1, ResolveError};
use age::{Identity, IdentityFile, NoCallbacks, Recipient};

const PLUGIN_NAME: &str = "pq";
const PLUGIN_BIN: &str = "age-plugin-pq";

pub(super) fn is_secret(line: &str) -> bool {
    line.to_ascii_uppercase().starts_with("AGE-SECRET-KEY-PQ-1")
}

pub(super) fn is_recipient(line: &str) -> bool {
    line.to_ascii_lowercase().starts_with("age1pq1")
}

pub(super) fn identities(hydrated: &str) -> Result<Vec<Box<dyn Identity + Send + Sync>>, String> {
    let ids = plugin_identity_file(hydrated)?
        .into_identities()
        .map_err(|e| e.to_string())?;
    non_empty(ids)
}

pub(super) fn recipients(hydrated: &str) -> Result<Vec<Box<dyn Recipient + Send>>, String> {
    let rs = plugin_identity_file(hydrated)?
        .to_recipients()
        .map_err(|e| e.to_string())?;
    non_empty(rs)
}

pub(super) fn public_recipient(line: &str) -> Result<Box<dyn Recipient + Send>, String> {
    let r: age::plugin::Recipient = line
        .parse()
        .map_err(|e| format!("invalid age1pq1 recipient: {e}"))?;
    RecipientPluginV1::new(PLUGIN_NAME, &[r], &[], NoCallbacks)
        .map(|p| Box::new(p) as Box<dyn Recipient + Send>)
        .map_err(|e| match e {
            ResolveError::MissingPlugin { .. } => missing_plugin(),
            other => other.to_string(),
        })
}

fn non_empty<T>(items: Vec<T>) -> Result<Vec<T>, String> {
    if items.is_empty() {
        return Err(format!("{PLUGIN_BIN} -identity returned no identity"));
    }
    Ok(items)
}

fn missing_plugin() -> String {
    format!(
        "{PLUGIN_BIN} not found on PATH. Post-quantum keys (age1pq1, AGE-SECRET-KEY-PQ-1) \
         need it: install age 1.3.2 or later from its release or mise"
    )
}

// Linux can shrink a pipe to one page. Staying under it lets the whole input
// sit in the pipe before the child runs.
const MAX_INPUT: usize = 4096;

fn plugin_identity_file(hydrated: &str) -> Result<IdentityFile<NoCallbacks>, String> {
    let keys: String = hydrated
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| format!("{l}\n"))
        .collect();
    if keys.len() > MAX_INPUT {
        return Err(format!(
            "post-quantum identity is {} bytes, over the {MAX_INPUT} bytes {PLUGIN_BIN} takes here",
            keys.len()
        ));
    }
    // main() restores the default SIGPIPE, so a child that exits before
    // reading must not find us mid-write: fill its stdin before the spawn.
    let (stdin, mut writer) =
        std::io::pipe().map_err(|e| format!("failed to open a pipe for {PLUGIN_BIN}: {e}"))?;
    writer
        .write_all(keys.as_bytes())
        .map_err(|e| format!("failed to pass the key to {PLUGIN_BIN}: {e}"))?;
    drop(writer);
    let output = Command::new(PLUGIN_BIN)
        .arg("-identity")
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => missing_plugin(),
            _ => format!("failed to run {PLUGIN_BIN}: {e}"),
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "{PLUGIN_BIN} -identity failed ({}): {}",
            output.status,
            stderr.trim()
        ));
    }
    IdentityFile::from_buffer(Cursor::new(output.stdout))
        .map_err(|e| format!("{PLUGIN_BIN} -identity output: {e}"))
}

/// Holds temp_env's lock, so a test that swaps PATH cannot run while this
/// one looks up age binaries on it.
#[cfg(test)]
pub(super) fn with_stable_path<T>(f: impl FnOnce() -> T) -> T {
    temp_env::with_vars::<&str, &str, _, _>([], f)
}

/// `(AGE-SECRET-KEY-PQ-1…, age1pq1…)` from Go `age-keygen -pq`.
#[cfg(test)]
pub(super) fn test_keygen() -> (String, String) {
    let out = Command::new("age-keygen")
        .arg("-pq")
        .output()
        .expect("age-keygen from age 1.3.2 on PATH, pinned in lade.yml");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let secret = text
        .lines()
        .find(|l| is_secret(l))
        .expect("age-keygen -pq prints a PQ secret")
        .to_string();
    let recipient = text
        .lines()
        .find_map(|l| l.strip_prefix("# public key: "))
        .expect("age-keygen -pq prints the public key")
        .to_string();
    assert!(is_recipient(&recipient), "{recipient}");
    (secret, recipient)
}
