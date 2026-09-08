use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::{Result, anyhow, bail};
use async_trait::async_trait;
use rustc_hash::FxHashMap;
use serde::Deserialize;
use url::Url;

use crate::Hydration;

use super::{Provider, Transport, Warnings, add_url, deserialize_output, run_cli};

const DOCS: &str = "https://bitwarden.com/help/cli/";

#[derive(Default)]
pub struct Bitwarden {
    urls: FxHashMap<Url, String>,
}

impl Bitwarden {
    pub fn new() -> Self {
        Default::default()
    }
}

#[derive(Deserialize)]
struct BwItem {
    id: String,
    name: String,
    #[serde(default)]
    notes: Option<String>,
    #[serde(default)]
    login: Option<BwLogin>,
    #[serde(default)]
    fields: Vec<BwField>,
}

#[derive(Deserialize)]
struct BwLogin {
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    totp: Option<String>,
}

#[derive(Deserialize)]
struct BwField {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    value: Option<String>,
}

fn decode_seg(raw: &str) -> Result<String> {
    urlencoding::decode(raw)
        .map_err(|e| anyhow!("invalid percent-encoding in bw:// URL: {e}"))
        .map(|s| s.into_owned())
}

fn item_and_field(url: &Url) -> Result<(String, String)> {
    let item = decode_seg(
        url.host_str()
            .ok_or_else(|| anyhow!("Bitwarden URI must be bw://<item>/<field>"))?,
    )?;
    if item.is_empty() {
        bail!("Bitwarden URI must be bw://<item>/<field>");
    }
    let field = url.path().trim_start_matches('/');
    let field = if field.is_empty() {
        "password".to_string()
    } else {
        decode_seg(field)?
    };
    if field.is_empty() || field.contains('/') {
        bail!("Bitwarden URI must be bw://<item>/<field>");
    }
    Ok((item, field))
}

fn pick_item<'a>(items: &'a [BwItem], needle: &str) -> Result<&'a BwItem> {
    let by_id: Vec<_> = items.iter().filter(|item| item.id == needle).collect();
    if by_id.len() == 1 {
        return Ok(by_id[0]);
    }
    let exact: Vec<_> = items.iter().filter(|item| item.name == needle).collect();
    match exact.as_slice() {
        [item] => Ok(*item),
        [] => bail!("Bitwarden item '{needle}' not found"),
        _ => bail!("Bitwarden item '{needle}' is not unique"),
    }
}

fn field_value(item: &BwItem, field: &str) -> Result<String> {
    let login = item.login.as_ref();
    let value = match field {
        "password" => login.and_then(|login| login.password.clone()),
        "username" => login.and_then(|login| login.username.clone()),
        "notes" => item.notes.clone(),
        "totp" => login.and_then(|login| login.totp.clone()),
        _ => item
            .fields
            .iter()
            .find(|row| row.name.as_deref() == Some(field))
            .and_then(|row| row.value.clone()),
    };
    value.ok_or_else(|| {
        anyhow!(
            "Bitwarden field '{field}' not found on item '{}'",
            item.name
        )
    })
}

#[async_trait]
impl Provider for Bitwarden {
    fn add(&mut self, value: String) -> Result<()> {
        add_url(&mut self.urls, value.clone(), "bw")?;
        let url = Url::parse(&value)?;
        item_and_field(&url)?;
        Ok(())
    }

    fn name(&self) -> &'static str {
        "Bitwarden"
    }

    fn install_url(&self) -> &'static str {
        DOCS
    }

    fn transport(&self) -> Transport {
        Transport::Cli
    }

    fn batch_unit(&self) -> &'static str {
        "vault"
    }

    fn has_work(&self) -> bool {
        !self.urls.is_empty()
    }

    async fn resolve(
        &self,
        _: &Path,
        extra_env: &HashMap<String, String>,
        _: &Warnings,
    ) -> Result<Hydration> {
        let extra_env = Arc::new(extra_env.clone());
        let child = run_cli(
            &["bw", "list", "items"],
            &extra_env,
            self.name(),
            self.install_url(),
            None,
        )
        .await?;
        if !child.status.success() {
            let stderr = String::from_utf8_lossy(&child.stderr);
            bail!("Bitwarden error: {stderr}");
        }
        let items: Vec<BwItem> = deserialize_output(&child, self.name())?;
        let mut hydration = Hydration::default();
        for (url, raw) in &self.urls {
            let (item, field) = item_and_field(url)?;
            let item = pick_item(&items, &item)?;
            hydration.insert(raw.clone(), field_value(item, &field)?);
        }
        Ok(hydration)
    }
}

#[cfg(test)]
mod tests;
