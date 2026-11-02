use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use lade_sdk::hydrate_one;
use serde_json::{Value, json};

use crate::audience::Via;
use crate::config::{Audience, RuleTtl};
use crate::event::{self, Emit, Kind};
use crate::hub::EvalCache;

/// `ttl`: keep the value in the hub, keyed by this cwd and URI. `None`
/// never touches the hub.
pub async fn resolve_uri(
    uri: String,
    cwd: &Path,
    command: &str,
    via: Via,
    audience: Audience,
    ttl: Option<&RuleTtl>,
) -> Result<String> {
    let started = Instant::now();
    let cache = match ttl {
        Some(ttl) => EvalCache::open(cwd, &uri, ttl, &crate::config::saved_user().await?)?,
        None => None,
    };
    if let Some(value) = cache.as_ref().and_then(EvalCache::lookup) {
        emit_fetch(command, &uri, cwd, via, audience, started, true);
        return Ok(value);
    }
    let value = hydrate_one(uri.clone(), cwd, &HashMap::new()).await?;
    if let Some(cache) = &cache {
        cache.store(&value);
    }
    emit_fetch(command, &uri, cwd, via, audience, started, false);
    Ok(value)
}

fn emit_fetch(
    command: &str,
    uri: &str,
    cwd: &Path,
    via: Via,
    audience: Audience,
    started: Instant,
    cached: bool,
) {
    let binding = if cached {
        json!({ "uri": uri, "cached": true })
    } else {
        json!({ "uri": uri })
    };
    event::emit(Emit {
        kind: Kind::Access,
        via,
        audience,
        actor: event::actor(&None),
        cwd: cwd.to_path_buf(),
        command: command.to_string(),
        argv: Some(json!([uri])),
        hydrated: None,
        matches: json!([{ "bindings": [binding] }]),
        hydrate_ms: Some(started.elapsed().as_secs_f64() * 1000.0),
        agent: crate::agent_meta::merge(Value::Null),
    });
}
