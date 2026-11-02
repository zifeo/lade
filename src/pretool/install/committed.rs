use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::{Map, Value, json};

use super::agent::Agent;
use super::merge::{command_is_lade_hook, parse_root, strip_matcher_hooks, to_pretty};

/// The Lade hooks of a repo's committed `.claude/settings.json`: the repo's
/// requirement, which Lade never moves or strips. Claude reads that file only
/// in a session started at the repo root, so the repo plane copies these
/// hooks into `settings.local.json`. Claude runs a command found in both files
/// once, with the fields of the local copy, so the copy is verbatim:
/// `onFailure` included.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Committed {
    pub path: PathBuf,
    /// `(matcher, hook)`, in file order. `None`: an entry with no matcher.
    pub hooks: Vec<(Option<String>, Value)>,
}

pub(super) fn committed_path(dest: &Path) -> PathBuf {
    dest.join(".claude").join("settings.json")
}

/// `None`: no file, or no Lade hook in it. Invalid JSON is an error.
pub(super) fn committed(dest: &Path) -> Result<Option<Committed>> {
    let path = committed_path(dest);
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("cannot read {}", path.display())),
    };
    let root =
        parse_root(&content, Agent::Claude).with_context(|| format!("in {}", path.display()))?;
    let hooks = lade_hooks(&root);
    if hooks.is_empty() {
        return Ok(None);
    }
    Ok(Some(Committed { path, hooks }))
}

impl Committed {
    /// Every committed Lade hook blocks the tool when it cannot run.
    pub(super) fn blocking(&self) -> bool {
        self.hooks
            .iter()
            .all(|(_, hook)| hook.get("onFailure").and_then(Value::as_str) == Some("block"))
    }

    pub(super) fn is_copied_in(&self, existing: &str) -> Result<bool> {
        if existing.trim().is_empty() {
            return Ok(false);
        }
        let local = lade_hooks(&parse_root(existing, Agent::Claude)?);
        Ok(local.len() == self.hooks.len() && self.hooks.iter().all(|pair| local.contains(pair)))
    }

    /// `existing` with its Lade hooks replaced by the committed ones. Other
    /// hooks and keys stay.
    pub(super) fn copy_into(&self, existing: &str) -> Result<String> {
        let mut root = parse_root(existing, Agent::Claude)?;
        let arr = root
            .as_object_mut()
            .context("Claude Code config must be a JSON object")?
            .entry("hooks")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .context("\"hooks\" must be an object")?
            .entry("PreToolUse")
            .or_insert_with(|| json!([]))
            .as_array_mut()
            .context("\"PreToolUse\" must be an array")?;
        strip_matcher_hooks(arr);
        for (matcher, hook) in &self.hooks {
            let entry = arr
                .iter_mut()
                .find(|entry| entry.get("matcher").and_then(Value::as_str) == matcher.as_deref());
            match entry.and_then(|entry| entry.get_mut("hooks").and_then(Value::as_array_mut)) {
                Some(hooks) => hooks.push(hook.clone()),
                None => {
                    let mut entry = Map::new();
                    if let Some(matcher) = matcher {
                        entry.insert("matcher".to_string(), json!(matcher));
                    }
                    entry.insert("hooks".to_string(), json!([hook]));
                    arr.push(Value::Object(entry));
                }
            }
        }
        to_pretty(&root)
    }
}

fn lade_hooks(root: &Value) -> Vec<(Option<String>, Value)> {
    let Some(entries) = root.pointer("/hooks/PreToolUse").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries {
        let matcher = entry
            .get("matcher")
            .and_then(Value::as_str)
            .map(str::to_string);
        for hook in entry
            .get("hooks")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|hook| command_is_lade_hook(hook))
        {
            found.push((matcher.clone(), hook.clone()));
        }
    }
    found
}
