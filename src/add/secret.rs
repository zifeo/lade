use anyhow::{Result, bail};
use lade_sdk::{Providers, secret_add_fields, secret_search_scope};
use std::collections::HashMap;

use super::ask;
use super::cli::{locked_path_env, login_stop, pick_from_lines};
use super::warn_raw;

pub fn secret_uri() -> Result<String> {
    let scheme = ask("Scheme (op, vault, awssm, azurekv, gcpsm, raw, …): ")?;
    if scheme.is_empty() {
        bail!("a scheme is required");
    }
    if scheme == "raw" || scheme == "file" || scheme == "age" {
        if scheme == "raw" {
            warn_raw();
        }
        return ask("Value or URI: ");
    }
    if let Some(uri) = pick_provider_secret(&scheme)? {
        return Ok(uri);
    }
    super::require_or_ask(None, &format!("URI ({scheme}://…): "), true)
}

fn pick_provider_secret(scheme: &str) -> Result<Option<String>> {
    let providers = Providers::new();
    let Some(provider) = providers.provider(scheme) else {
        return Ok(None);
    };
    let bin = lade_sdk::compat::spec_for(scheme)
        .map(|spec| spec.bin)
        .unwrap_or(scheme);
    let mut extra_env = locked_path_env(bin)?;
    let mut extras = HashMap::new();
    if let Some(field) = secret_search_scope(scheme) {
        let value = ask_field(&field)?;
        extra_env.insert("LADE_SEARCH_SCOPE".to_string(), value.clone());
        extras.insert(field.key.to_string(), value);
    }
    let hits = match lade_sdk::Provider::search(provider, &extra_env) {
        Ok(hits) if !hits.is_empty() => hits,
        Ok(_) => return Ok(None),
        Err(_) => return login_stop(bin),
    };
    let picked = pick_from_lines(lade_sdk::Provider::name(provider), &hits)?;
    for field in secret_add_fields(scheme) {
        extras.insert(field.key.to_string(), ask_field(field)?);
    }
    Ok(Some(
        lade_sdk::compose_secret_add_uri(scheme, &picked, &extras)
            .unwrap_or_else(|| format!("{scheme}://{picked}")),
    ))
}

fn ask_field(field: &lade_sdk::AddField) -> Result<String> {
    let answer = ask(field.prompt)?;
    if !answer.is_empty() {
        return Ok(answer);
    }
    match field.default {
        Some(default) => Ok(default.to_string()),
        None => bail!("{} is required", field.key),
    }
}
