use super::*;
use crate::audience::Via;
use crate::config::Audience;
use serde_json::{Value, json};
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
