use std::{collections::HashMap, io::Read, path::Path};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use futures::future::try_join_all;
use rustc_hash::FxHashMap;

use crate::Hydration;

use super::params::{
    Query, apply_process_path, load_named_identity, parse_plugin_name, require_age_plugin_on_path,
    require_identity_names, split_scheme_path_query,
};
use super::{Provider, Transport, Warnings};

const DOCS: &str = "https://github.com/FiloSottile/age";

#[derive(Default)]
pub struct Age {
    values: FxHashMap<String, String>,
}

impl Age {
    pub fn new() -> Self {
        Default::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AgeUri {
    plugin: Option<String>,
    query: Query,
    blob: Vec<u8>,
}

fn decode_blob(raw: &str) -> Result<Vec<u8>> {
    Ok(urlencoding::decode_binary(raw.as_bytes()).into_owned())
}

fn parse_age(value: &str) -> Result<AgeUri> {
    let (path, query) = split_scheme_path_query(value, "age")?;
    let blob = decode_blob(&path)?;
    if blob.is_empty() {
        bail!("age:// ciphertext cannot be empty");
    }
    let plugin = match query.get("plugin") {
        Some(name) => Some(parse_plugin_name(name)?),
        None => None,
    };
    require_identity_names(&query, plugin.as_deref(), DOCS)?;
    Ok(AgeUri {
        plugin,
        query,
        blob,
    })
}

fn identity_for(uri: &AgeUri, extra_env: &HashMap<String, String>) -> Result<String> {
    if let Some(plugin) = &uri.plugin {
        require_age_plugin_on_path(extra_env, plugin, DOCS)?;
    }
    load_named_identity(
        extra_env,
        &uri.query,
        &["LADE_AGE_KEY"],
        &["LADE_AGE_KEY_FILE"],
        "age",
        DOCS,
    )
}

fn decrypt_blob(identity: &str, blob: &[u8]) -> Result<String> {
    let mut cursor = std::io::Cursor::new(identity.as_bytes());
    let identities = match ::age::ssh::Identity::from_buffer(&mut cursor, None) {
        Ok(ssh) => vec![Box::new(ssh) as Box<dyn ::age::Identity>],
        Err(_) => {
            cursor.set_position(0);
            ::age::IdentityFile::from_buffer(cursor)
                .map_err(|e| anyhow!("age identity parse failed: {e}. See {DOCS}."))?
                .into_identities()
                .map_err(|e| anyhow!("age identity parse failed: {e}. See {DOCS}."))?
        }
    };
    let decryptor = ::age::Decryptor::new(::age::armor::ArmoredReader::new(blob))
        .map_err(|e| anyhow!("age decrypt failed: {e}. See {DOCS}."))?;
    let mut reader = decryptor
        .decrypt(
            identities
                .iter()
                .map(|i| i.as_ref() as &dyn ::age::Identity),
        )
        .map_err(|e| anyhow!("age decrypt failed: {e}. See {DOCS}."))?;
    let mut out = Vec::new();
    reader
        .read_to_end(&mut out)
        .map_err(|e| anyhow!("age decrypt failed: {e}. See {DOCS}."))?;
    String::from_utf8(out).map_err(|e| anyhow!("age decrypt failed: {e}. See {DOCS}."))
}

#[async_trait]
impl Provider for Age {
    fn add(&mut self, value: String) -> Result<()> {
        let rest = value
            .strip_prefix("age://")
            .ok_or_else(|| anyhow!("Not an age scheme"))?;
        if rest.is_empty() || rest.starts_with('?') {
            bail!("age:// ciphertext cannot be empty");
        }
        self.values.insert(value.clone(), value);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "age"
    }

    fn install_url(&self) -> &'static str {
        DOCS
    }

    fn transport(&self) -> Transport {
        Transport::Sdk
    }

    fn batch_unit(&self) -> &'static str {
        "(plugin, identity, ciphertext)"
    }

    fn has_work(&self) -> bool {
        !self.values.is_empty()
    }

    async fn resolve(
        &self,
        _: &Path,
        extra_env: &HashMap<String, String>,
        _: &Warnings,
    ) -> Result<Hydration> {
        apply_process_path(extra_env);
        let mut native: HashMap<(Option<String>, String, Vec<u8>), Vec<String>> = HashMap::new();
        let mut plugins: HashMap<(Option<String>, String, Vec<u8>), Vec<String>> = HashMap::new();
        for raw in self.values.keys() {
            let uri = parse_age(raw)?;
            let identity = identity_for(&uri, extra_env)?;
            let dest = if uri.plugin.is_some() {
                &mut plugins
            } else {
                &mut native
            };
            dest.entry((uri.plugin, identity, uri.blob))
                .or_default()
                .push(raw.clone());
        }
        let native_fetches = native
            .into_iter()
            .map(
                |((_, identity, blob), uris)| async move { hydrate_plain(&identity, &blob, uris) },
            );
        let mut hydration: Hydration = try_join_all(native_fetches)
            .await?
            .into_iter()
            .flatten()
            .collect();
        for ((_, identity, blob), uris) in plugins {
            hydration.extend(hydrate_plain(&identity, &blob, uris)?);
        }
        Ok(hydration)
    }
}

fn hydrate_plain(identity: &str, blob: &[u8], uris: Vec<String>) -> Result<Hydration> {
    let plain = decrypt_blob(identity, blob)?;
    let mut hydration = Hydration::default();
    for uri in uris {
        hydration.insert(uri, plain.clone());
    }
    Ok(hydration)
}

#[cfg(test)]
mod tests;
