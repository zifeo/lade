use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use lade_sdk::hydrate_one;
use serde_json::{Value, json};

use crate::audience::Via;
use crate::config::Audience;
use crate::event::{self, Emit, Kind};

pub async fn resolve_uri(
    uri: String,
    cwd: &Path,
    command: &str,
    via: Via,
    audience: Audience,
) -> Result<String> {
    let started = Instant::now();
    let value = hydrate_one(uri.clone(), cwd, &HashMap::new()).await?;
    emit_fetch(command, &uri, cwd, via, audience, started);
    Ok(value)
}

fn emit_fetch(
    command: &str,
    uri: &str,
    cwd: &Path,
    via: Via,
    audience: Audience,
    started: Instant,
) {
    event::emit(Emit {
        kind: Kind::Access,
        via,
        audience,
        actor: event::actor(&None),
        cwd: cwd.to_path_buf(),
        command: command.to_string(),
        argv: Some(json!([uri])),
        hydrated: None,
        matches: json!([{ "bindings": [{ "uri": uri }] }]),
        hydrate_ms: Some(started.elapsed().as_secs_f64() * 1000.0),
        agent: crate::agent_meta::merge(Value::Null),
    });
}
