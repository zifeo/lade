//! OpenCode 1.x calls every function a plugin file exports. 2.x wants a
//! default export with `id` and `setup`. One file cannot serve both, so the
//! plugin follows `opencode --version` when Lade writes it, and keeps its
//! API on refresh: a repo file shared by teammates must not flip.

use std::sync::OnceLock;

use anyhow::{Context, Result, bail};

const V1: &str = include_str!("opencode_v1.js");
const V2: &str = include_str!("../../../.opencode/plugins/lade-pretool.js");

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OpenCodeApi {
    V1,
    V2,
}

impl OpenCodeApi {
    pub(super) fn snapshot(self) -> &'static str {
        match self {
            OpenCodeApi::V1 => V1,
            OpenCodeApi::V2 => V2,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            OpenCodeApi::V1 => "v1",
            OpenCodeApi::V2 => "v2",
        }
    }

    /// API of a Lade plugin file. `None` when the file is not ours.
    pub(super) fn of(content: &str) -> Option<Self> {
        if content.contains(r#"id: "lade-pretool""#) {
            return Some(OpenCodeApi::V2);
        }
        // The markers Lade 0.18 to 0.19.2 match. The v2 file avoids them.
        let v1 = content.contains("LadePretool")
            || content.contains("lade hook")
            || content.contains("[\"hook\"]")
            || content.contains("[\"hook\",");
        v1.then_some(OpenCodeApi::V1)
    }

    /// From `opencode --version` on PATH. Run once per process.
    pub(crate) fn installed() -> Result<Self> {
        static API: OnceLock<std::result::Result<OpenCodeApi, String>> = OnceLock::new();
        API.get_or_init(|| detect().map_err(|e| format!("{e:#}")))
            .clone()
            .map_err(anyhow::Error::msg)
    }
}

fn detect() -> Result<OpenCodeApi> {
    let output = std::process::Command::new("opencode")
        .arg("--version")
        .output()
        .context(
            "`opencode` is not on PATH. Lade picks the OpenCode plugin API from `opencode --version`",
        )?;
    if !output.status.success() {
        bail!(
            "`opencode --version` failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    api_from_version(&String::from_utf8_lossy(&output.stdout))
}

/// `1.19.0` and `opencode v2.0.29` are both real outputs.
pub(super) fn api_from_version(out: &str) -> Result<OpenCodeApi> {
    let major = out
        .split_whitespace()
        .find_map(|token| {
            let version = token.trim_start_matches('v');
            let (major, _) = version.split_once('.')?;
            major.parse::<u64>().ok()
        })
        .with_context(|| {
            format!(
                "cannot read a version from `opencode --version`: {}",
                out.trim()
            )
        })?;
    Ok(if major >= 2 {
        OpenCodeApi::V2
    } else {
        OpenCodeApi::V1
    })
}

/// The `api` plugin, pinned to the hook binary when it is not plain `lade`.
pub(super) fn plugin(api: OpenCodeApi, command: &str) -> String {
    instantiate_plugin(api.snapshot(), command)
}

const BIN_PREFIX: &str = r#"process.env.LADE_BIN ?? ""#;

pub(super) fn instantiate_plugin(snapshot: &str, command: &str) -> String {
    let bin = command
        .split_whitespace()
        .next()
        .unwrap_or("lade")
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    pin(snapshot, &bin)
}

/// `bin` is already escaped for a JS string.
fn pin(snapshot: &str, bin: &str) -> String {
    if bin == "lade" {
        return snapshot.to_string();
    }
    snapshot.replace(
        &format!("{BIN_PREFIX}lade\""),
        &format!("{BIN_PREFIX}{bin}\""),
    )
}

/// One of our snapshots, whatever binary it pins. Refresh leaves any other
/// content alone: it was edited by hand.
pub(super) fn is_pristine(content: &str) -> bool {
    let (Some(api), Some(bin)) = (OpenCodeApi::of(content), pinned_bin(content)) else {
        return false;
    };
    content.trim_end() == pin(api.snapshot(), bin).trim_end()
}

fn pinned_bin(content: &str) -> Option<&str> {
    let start = content.find(BIN_PREFIX)? + BIN_PREFIX.len();
    let rest = &content[start..];
    let mut escaped = false;
    for (index, c) in rest.char_indices() {
        match (escaped, c) {
            (true, _) => escaped = false,
            (false, '\\') => escaped = true,
            (false, '"') => return Some(&rest[..index]),
            (false, _) => {}
        }
    }
    None
}
