//! Best-effort agent metadata. Missing or unknown fields stay absent.
//! The diary stores the object as-is. New keys do not need a schema change.

use serde_json::{Map, Value, json};

use crate::audience;

/// Overlay env fallbacks onto hook-collected metadata. Hook keys win.
pub fn merge(base: Value) -> Option<Value> {
    let mut obj = match base {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    if let Some(env) = from_env().as_object() {
        for (key, value) in env {
            obj.entry(key.clone()).or_insert(value.clone());
        }
    }
    if obj.is_empty() {
        None
    } else {
        Some(Value::Object(obj))
    }
}

pub fn from_env() -> Value {
    let mut obj = Map::new();
    if let Some(name) = audience::agent_signal() {
        obj.insert("harness".to_string(), json!(name));
    }
    if let Some(model) = pinned(std::env::var("PI_MODEL").ok().as_deref()) {
        obj.insert("model".to_string(), json!(model));
    }
    if let Some(session) = nonempty("PI_SESSION_ID").or_else(|| nonempty("CODEX_THREAD_ID")) {
        obj.insert("session".to_string(), json!(session));
    }
    if obj.is_empty() {
        Value::Null
    } else {
        Value::Object(obj)
    }
}

pub(crate) fn pinned(raw: Option<&str>) -> Option<String> {
    let value = raw?.trim();
    if value.is_empty() {
        return None;
    }
    match value.to_ascii_lowercase().as_str() {
        "auto" | "default" | "inherit" => None,
        _ => Some(value.to_string()),
    }
}

fn nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_hook_keys() {
        temp_env::with_vars(
            [
                ("AI_AGENT", Some("pi")),
                ("PI_MODEL", Some("opus")),
                ("PI_SESSION_ID", Some("sess")),
                ("AGENT", None),
                ("CLAUDECODE", None),
                ("CURSOR_AGENT", None),
                ("COPILOT_MODEL", None),
            ],
            || {
                let merged = merge(json!({"harness": "cursor", "model": "grok"})).unwrap();
                assert_eq!(merged["harness"], "cursor");
                assert_eq!(merged["model"], "grok");
                assert_eq!(merged["session"], "sess");
            },
        );
    }

    #[test]
    fn empty_is_none() {
        temp_env::with_vars(
            [
                ("AI_AGENT", None::<&str>),
                ("AGENT", None),
                ("CLAUDECODE", None),
                ("CURSOR_AGENT", None),
                ("COPILOT_MODEL", None),
                ("PI_MODEL", None),
                ("PI_SESSION_ID", None),
                ("CLAUDE_CODE", None),
                ("CURSOR_EXTENSION_HOST_ROLE", None),
                ("CURSOR_SANDBOX", None),
                ("CODEX_THREAD_ID", None),
                ("CODEX_SANDBOX", None),
                ("CODEX_CI", None),
                ("OPENCODE", None),
                ("OPENCODE_PID", None),
            ],
            || assert_eq!(merge(Value::Null), None),
        );
    }

    #[test]
    fn auto_model_is_dropped() {
        assert_eq!(pinned(Some("auto")), None);
        assert_eq!(pinned(Some("DEFAULT")), None);
        assert_eq!(pinned(Some("  ")), None);
        assert_eq!(pinned(Some("grok-4.5")).as_deref(), Some("grok-4.5"));
    }

    #[test]
    fn from_env_reads_codex_thread() {
        temp_env::with_vars(
            [
                ("AI_AGENT", None::<&str>),
                ("AGENT", None),
                ("CLAUDECODE", None),
                ("CURSOR_AGENT", None),
                ("COPILOT_MODEL", None),
                ("PI_MODEL", None),
                ("PI_SESSION_ID", None),
                ("CLAUDE_CODE", None),
                ("CURSOR_EXTENSION_HOST_ROLE", None),
                ("CURSOR_SANDBOX", None),
                ("CODEX_THREAD_ID", Some("thr_9")),
                ("CODEX_SANDBOX", None),
                ("CODEX_CI", None),
                ("OPENCODE", None),
                ("OPENCODE_PID", None),
            ],
            || {
                let env = from_env();
                assert_eq!(env["harness"], "codex");
                assert_eq!(env["session"], "thr_9");
            },
        );
    }
}
