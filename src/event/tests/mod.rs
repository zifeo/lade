use super::*;
use crate::audience::Via;
use crate::config::Audience;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::PathBuf;

mod path;
mod verb;
mod verify;

fn verb_emit(command: &str, argv: Option<Value>, agent: Value) -> Emit {
    Emit {
        kind: Kind::Seen,
        via: Via::Mcp,
        audience: Audience::Agent,
        actor: None,
        cwd: PathBuf::from("."),
        command: command.into(),
        argv,
        hydrated: None,
        matches: json!([]),
        hydrate_ms: None,
        agent: Some(agent),
    }
}

fn seen_emit(command: &str) -> Emit {
    Emit {
        kind: Kind::Seen,
        via: Via::Organic,
        audience: Audience::Human,
        actor: None,
        cwd: PathBuf::from("."),
        command: command.into(),
        argv: None,
        hydrated: None,
        matches: json!([]),
        hydrate_ms: None,
        agent: None,
    }
}

#[test]
fn mark_cached_sets_only_hit_keys() {
    let mut matches = json!([{
        "file": "/tmp/lade.yaml",
        "rule": "terraform .*",
        "bindings": [
            {"key": "AWS_ACCESS_KEY_ID", "uri": "op://v/i/f", "family": "secret"},
            {"key": "REGION", "uri": "op://v/i/r", "family": "secret"}
        ]
    }]);
    let mut keys = HashSet::new();
    keys.insert("AWS_ACCESS_KEY_ID".into());
    mark_cached(&mut matches, &keys);
    assert_eq!(matches[0]["bindings"][0]["cached"], json!(true));
    assert!(matches[0]["bindings"][1].get("cached").is_none());
}
