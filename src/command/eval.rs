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
    // § xrjs58t2q.b@5m0cc
    let started = Instant::now();
    let user = crate::config::saved_user().await?;
    let cache = match ttl {
        Some(ttl) => EvalCache::open(cwd, &uri, ttl, &user)?,
        None => None,
    };
    // § one xrjs58t2q.c@zhwh5
    if crate::family::Family::of_uri(&uri) != crate::family::Family::Secret {
        anyhow::bail!(
            "lade eval resolves a secret URI, and {uri} names a tunnel or a package. Bind it in a lade.yaml rule and run the command with lade --."
        );
    }
    // § gb0y44ydb.b@7tmrt
    if let Some(value) = cache.as_ref().and_then(EvalCache::lookup) {
        // § hadg0brg4.a@gtdmx
        emit_fetch(command, &uri, cwd, via, audience, started, true, &user);
        return Ok(value);
    }
    let value = hydrate_one(uri.clone(), cwd, &HashMap::new()).await?;
    if let Some(cache) = &cache {
        cache.store(&value);
    }
    emit_fetch(command, &uri, cwd, via, audience, started, false, &user);
    // § end gb0y44ydb.b
    Ok(value)
}

#[allow(clippy::too_many_arguments)]
fn emit_fetch(
    command: &str,
    uri: &str,
    cwd: &Path,
    via: Via,
    audience: Audience,
    started: Instant,
    cached: bool,
    user: &Option<String>,
) {
    // § one cj4kjwcp7.b@634r1, ezvgtge5j.a@1dqty
    let binding = if cached {
        json!({ "uri": uri, "cached": true })
    } else {
        json!({ "uri": uri })
    };
    event::emit(Emit {
        kind: Kind::Access,
        via,
        audience,
        actor: event::actor(user),
        cwd: cwd.to_path_buf(),
        command: command.to_string(),
        argv: Some(json!([uri])),
        hydrated: None,
        matches: json!([{ "bindings": [binding] }]),
        hydrate_ms: Some(started.elapsed().as_secs_f64() * 1000.0),
        agent: crate::agent_meta::merge(Value::Null),
    });
}
