use anyhow::{Context, Result, bail};
use lade_sdk::Providers;
use std::collections::HashMap;

use super::ask;
use super::cli::{family_program, login_stop, pick_from_lines};
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
    let scope = if scheme == "azurekv" {
        let vault = ask("Vault name: ")?;
        if vault.is_empty() {
            bail!("a vault name is required");
        }
        extra_env.insert("LADE_SEARCH_SCOPE".to_string(), vault.clone());
        Some(vault)
    } else {
        None
    };
    let hits = match lade_sdk::Provider::search(provider, &extra_env) {
        Ok(hits) if !hits.is_empty() => hits,
        Ok(_) => return Ok(None),
        Err(_) => return login_stop(bin),
    };
    let picked = pick_from_lines(lade_sdk::Provider::name(provider), &hits)?;
    Ok(Some(compose_secret_uri(scheme, &picked, scope.as_deref())?))
}

fn compose_secret_uri(scheme: &str, picked: &str, scope: Option<&str>) -> Result<String> {
    match scheme {
        "op" => {
            let field = ask_or("Field (password): ", "password")?;
            Ok(format!("op://{picked}/{field}"))
        }
        "vault" => {
            let field = ask_or("Field (password): ", "password")?;
            let host = ask_or("Vault host (127.0.0.1:8200): ", "127.0.0.1:8200")?;
            Ok(format!("vault://{host}/secret/{picked}/{field}"))
        }
        "awssm" => {
            let region = ask_or("Region (us-east-1): ", "us-east-1")?;
            Ok(format!("awssm://{region}/{picked}"))
        }
        "azurekv" => {
            let vault = scope.context("vault name")?;
            Ok(format!("azurekv://{vault}/{picked}"))
        }
        "gcpsm" => {
            let project = ask("Project: ")?;
            if project.is_empty() {
                bail!("a project is required");
            }
            Ok(format!("gcpsm://{project}/{picked}"))
        }
        _ => Ok(format!("{scheme}://{picked}")),
    }
}

fn ask_or(prompt: &str, default: &str) -> Result<String> {
    let answer = ask(prompt)?;
    Ok(if answer.is_empty() {
        default.to_string()
    } else {
        answer
    })
}

fn locked_path_env(bin: &str) -> Result<HashMap<String, String>> {
    let path = family_program(bin)?;
    let dir = path.parent().context("bin has no parent")?;
    let rest = std::env::var("PATH").unwrap_or_default();
    Ok(HashMap::from([(
        "PATH".to_string(),
        format!("{}:{rest}", dir.display()),
    )]))
}
