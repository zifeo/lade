mod loader;
mod secret;
#[cfg(test)]
mod tests;

pub use loader::LadeFile;
use secret::resolve_lade_secret;
pub use secret::*;

use crate::global_config::GlobalConfig;
use anyhow::{Result, bail};
use futures::future::try_join_all;
use lade_sdk::{hydrate_one, hydrate_with_maskable};
use regex::RegexSet;
use rustc_hash::FxHashMap;
use rustc_hash::FxHashSet;
use std::{collections::HashMap, path::PathBuf};

pub type Output = Option<PathBuf>;

type VarsByOutput = FxHashMap<Output, HashMap<String, String>>;

type CollectHydrateAccum = (
    VarsByOutput,
    HashMap<String, String>,
    FxHashSet<String>,
    Vec<String>,
);

fn output_name(output: &Output) -> String {
    output
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "environment".to_string())
}

fn merge_vars(
    vars: &mut VarsByOutput,
    output: Output,
    incoming: HashMap<String, String>,
) -> Result<()> {
    let target = vars.entry(output.clone()).or_default();
    for (key, value) in incoming {
        match target.get(&key) {
            Some(existing) if existing != &value => bail!(
                "conflicting value for '{}' in {}: '{}' and '{}' match the same command; use more specific rules",
                key,
                output_name(&output),
                existing,
                value
            ),
            Some(_) => {}
            None => {
                target.insert(key, value);
            }
        }
    }
    Ok(())
}

fn merge_sources(
    sources: &mut HashMap<String, String>,
    incoming: HashMap<String, String>,
) -> Result<()> {
    for (key, source) in incoming {
        match sources.get(&key) {
            Some(existing) if existing != &source => bail!(
                "conflicting source for '{}': '{}' and '{}' match the same command; use one source per variable",
                key,
                existing,
                source
            ),
            Some(_) => {}
            None => {
                sources.insert(key, source);
            }
        }
    }
    Ok(())
}

fn rule_sources(rule: &LadeRule, saved_user: &Option<String>) -> HashMap<String, String> {
    rule.secrets
        .iter()
        .filter_map(|(key, secret)| {
            resolve_lade_secret(secret, saved_user).map(|v| (key.clone(), v))
        })
        .collect()
}

async fn saved_user() -> Result<Option<String>> {
    use std::env;

    let local_config = GlobalConfig::load().await?;
    Ok(local_config
        .user
        .or_else(|| env::var("USER").ok().or_else(|| env::var("USERNAME").ok())))
}

pub struct Config {
    rules: Vec<(PathBuf, LadeRule)>,
    regex_set: RegexSet,
}

impl Config {
    pub(crate) fn new(rules: Vec<(PathBuf, LadeRule)>, regex_set: RegexSet) -> Self {
        Config { rules, regex_set }
    }

    pub(crate) fn collect(&self, command: &str) -> Vec<(PathBuf, LadeRule)> {
        self.regex_set
            .matches(command)
            .into_iter()
            .map(|i| self.rules[i].clone())
            .collect()
    }

    async fn hydrate_output(
        &self,
        path: PathBuf,
        rule: LadeRule,
        saved_user: &Option<String>,
    ) -> Result<(
        Output,
        HashMap<String, String>,
        HashMap<String, String>,
        FxHashSet<String>,
        Vec<String>,
    )> {
        let sources = rule_sources(&rule, saved_user);

        let config = rule.config.as_ref();
        let output = config.and_then(|c| c.file.clone());
        let extra_env = if let Some(uri) = config
            .and_then(|c| c.onepassword_service_account.as_ref())
            .and_then(|sa| resolve_lade_secret(sa, saved_user))
        {
            let token = hydrate_one(uri, &path, &HashMap::new()).await?;
            HashMap::from([("OP_SERVICE_ACCOUNT_TOKEN".to_string(), token)])
        } else {
            HashMap::new()
        };

        let (values, maskable, warnings) =
            hydrate_with_maskable(sources.clone(), path.clone(), extra_env).await?;
        Ok((
            output.map(|subpath| path.join(subpath)),
            values,
            sources,
            maskable,
            warnings,
        ))
    }

    pub async fn collect_hydrate(
        &self,
        command: &str,
    ) -> Result<(
        HashMap<Output, HashMap<String, String>>,
        HashMap<String, String>,
        FxHashSet<String>,
        Vec<String>,
    )> {
        let saved_user = saved_user().await?;

        let (vars, sources, maskable, warnings): CollectHydrateAccum = try_join_all(
            self.collect(command)
                .into_iter()
                .map(|(path, rule)| self.hydrate_output(path, rule, &saved_user)),
        )
        .await?
        .into_iter()
        .try_fold(
            (
                FxHashMap::default(),
                HashMap::new(),
                FxHashSet::default(),
                Vec::new(),
            ),
            |(mut vars, mut sources, mut maskable, mut warnings),
             (output, map, rule_sources, rule_maskable, rule_warnings)| {
                merge_vars(&mut vars, output, map)?;
                merge_sources(&mut sources, rule_sources)?;
                maskable.extend(rule_maskable);
                warnings.extend(rule_warnings);
                Ok::<_, anyhow::Error>((vars, sources, maskable, warnings))
            },
        )?;
        Ok((vars.into_iter().collect(), sources, maskable, warnings))
    }

    pub fn collect_keys(&self, command: &str) -> HashMap<Output, Vec<String>> {
        self.collect(command)
            .into_iter()
            .map(|(_, rule)| {
                (
                    rule.config.as_ref().and_then(|c| c.file.clone()),
                    rule.secrets.keys().cloned().collect::<Vec<_>>(),
                )
            })
            .collect()
    }

    pub fn collect_disclaimers(&self, command: &str) -> Vec<String> {
        self.collect(command)
            .into_iter()
            .filter_map(|(_, rule)| rule.config.as_ref().and_then(|c| c.disclaimer.clone()))
            .collect()
    }

    pub fn all_secret_sources(&self, saved_user: &Option<String>) -> Vec<String> {
        self.rules
            .iter()
            .flat_map(|(_, rule)| rule_sources(rule, saved_user).into_values())
            .collect()
    }

    pub fn rule_count(&self) -> usize {
        self.rules.len()
    }
}
