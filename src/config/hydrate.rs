use anyhow::{Result, bail};
use futures::stream::{FuturesUnordered, StreamExt};
use lade_sdk::{Dag, Template, hydrate_one, hydrate_with_maskable};
use rustc_hash::{FxHashMap, FxHashSet};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::PathBuf,
};

use super::resolve::{ResolvedEntry, binding_name, resolve_entry, split_scheme};
use super::secret::resolve_lade_secret;
use super::{Config, LadeRule, Output};
use crate::ticket::TicketSecret;

#[derive(Debug, Clone)]
struct Binding {
    private: bool,
    source: String,
    cwd: PathBuf,
    output: Output,
    extra_env: HashMap<String, String>,
}

fn is_shell_source(source: &str) -> bool {
    matches!(split_scheme(source), Some("sh" | "bash" | "zsh" | "fish"))
}

async fn bindings_from_rules(
    rules: &[(PathBuf, LadeRule)],
    saved_user: &Option<String>,
) -> Result<HashMap<String, Binding>> {
    let mut bindings = HashMap::<String, Binding>::new();
    for (cwd, rule) in rules {
        let output = rule.config.as_ref().and_then(|config| config.file.clone());
        let extra_env = if let Some(uri) = rule
            .config
            .as_ref()
            .and_then(|config| config.onepassword_service_account.as_ref())
            .and_then(|secret| resolve_lade_secret(secret, saved_user))
        {
            HashMap::from([(
                "OP_SERVICE_ACCOUNT_TOKEN".to_string(),
                hydrate_one(uri, cwd, &HashMap::new()).await?,
            )])
        } else {
            HashMap::new()
        };
        for (key, secret) in &rule.secrets {
            match resolve_entry(key, secret, saved_user) {
                Some(ResolvedEntry::Unset { key })
                | Some(ResolvedEntry::Network { key, .. })
                | Some(ResolvedEntry::Pin { key, .. }) => {
                    let (name, _) = binding_name(&key)?;
                    bindings.remove(&name);
                }
                Some(ResolvedEntry::InvalidNumericSecret { key }) => bail!(
                    "numeric key '{}' must use a network URI (kubectl://, kubefwd://, tsh://)",
                    key
                ),
                None => {}
                Some(ResolvedEntry::Secret { key, value }) => {
                    let (name, private) = binding_name(&key)?;
                    let binding = Binding {
                        private,
                        source: value,
                        cwd: cwd.clone(),
                        output: output.as_ref().map(|path| cwd.join(path)),
                        extra_env: extra_env.clone(),
                    };
                    if let Some(existing) = bindings.get(&name)
                        && existing.private != binding.private
                    {
                        bail!("binding '{name}' is declared both public and private");
                    }
                    bindings.insert(name, binding);
                }
            }
        }
    }
    Ok(bindings)
}

async fn bindings_from_ticket(
    secrets: &[TicketSecret],
    op_sa: Option<&str>,
) -> Result<HashMap<String, Binding>> {
    let mut op_tokens = HashMap::<PathBuf, String>::new();
    let mut bindings = HashMap::<String, Binding>::new();
    for secret in secrets {
        let extra_env = if let Some(uri) = op_sa {
            let token = if let Some(token) = op_tokens.get(&secret.cwd) {
                token.clone()
            } else {
                let token = hydrate_one(uri.to_string(), &secret.cwd, &HashMap::new()).await?;
                op_tokens.insert(secret.cwd.clone(), token.clone());
                token
            };
            HashMap::from([("OP_SERVICE_ACCOUNT_TOKEN".to_string(), token)])
        } else {
            HashMap::new()
        };
        if let Some(existing) = bindings.get(&secret.key)
            && existing.private != secret.private
        {
            bail!(
                "binding '{}' is declared both public and private",
                secret.key
            );
        }
        bindings.insert(
            secret.key.clone(),
            Binding {
                private: secret.private,
                source: secret.source.clone(),
                cwd: secret.cwd.clone(),
                output: secret.output.clone(),
                extra_env,
            },
        );
    }
    Ok(bindings)
}

async fn hydrate_bindings(
    bindings: HashMap<String, Binding>,
) -> Result<(
    HashMap<Output, HashMap<String, String>>,
    HashMap<String, String>,
    FxHashSet<String>,
    Vec<String>,
)> {
    let templates = bindings
        .iter()
        .map(|(name, binding)| (name.clone(), Template::parse(&binding.source)))
        .collect::<HashMap<_, _>>();
    let dag = Dag::new(templates)?;
    let mut degrees = dag.indegrees();
    let mut ready = dag.initial_ready();
    let mut values = HashMap::<String, String>::new();
    let mut sources = HashMap::<String, String>::new();
    let mut maskable = FxHashSet::default();
    let mut warnings = Vec::new();

    let mut running = FuturesUnordered::new();
    while !ready.is_empty() || !running.is_empty() {
        let batch = std::mem::take(&mut ready);
        let mut groups =
            BTreeMap::<(PathBuf, Vec<(String, String)>, bool), HashMap<String, String>>::new();
        for name in &batch {
            let binding = bindings.get(name).expect("planned binding");
            let template = dag.template(name).expect("planned template");
            let shell_source = is_shell_source(&binding.source);
            let rendered = if shell_source {
                template.shell_source()
            } else {
                template.render(&values)?
            };
            let mut extra_env = binding
                .extra_env
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect::<Vec<_>>();
            if shell_source {
                extra_env.extend(template.dependencies().filter_map(|dependency| {
                    values
                        .get(dependency)
                        .map(|value| (dependency.to_string(), value.clone()))
                }));
            }
            extra_env.sort();
            groups
                .entry((binding.cwd.clone(), extra_env, shell_source))
                .or_default()
                .insert(name.clone(), rendered);
        }
        for ((cwd, extra_env, _), sources_for_group) in groups {
            running.push(async move {
                let extra_env = extra_env.into_iter().collect::<HashMap<_, _>>();
                let configured = sources_for_group.clone();
                let result = hydrate_with_maskable(sources_for_group, cwd, extra_env).await?;
                Ok::<_, anyhow::Error>((configured, result))
            });
        }
        let (configured, (resolved, group_maskable, group_warnings)) = running
            .next()
            .await
            .expect("a planned DAG must have an active group")?;
        for (name, value) in resolved {
            let source = configured.get(&name).expect("configured source").clone();
            if group_maskable.contains(&source)
                || dag
                    .template(&name)
                    .expect("planned template")
                    .dependencies()
                    .any(|dependency| maskable.contains(dependency))
            {
                maskable.insert(name.clone());
            }
            if group_maskable.contains(&source) {
                maskable.insert(source.clone());
            }
            values.insert(name.clone(), value);
            sources.insert(name, source);
        }
        warnings.extend(group_warnings);
        let mut newly_ready = BTreeSet::new();
        for name in configured.keys() {
            for dependent in dag.dependents(name) {
                let degree = degrees.get_mut(dependent).expect("planned dependent");
                *degree -= 1;
                if *degree == 0 {
                    newly_ready.insert(dependent.clone());
                }
            }
        }
        ready.extend(newly_ready);
    }

    let mut vars = FxHashMap::<Output, HashMap<String, String>>::default();
    for (name, binding) in bindings {
        if binding.private {
            continue;
        }
        vars.entry(binding.output).or_default().insert(
            name.clone(),
            values.remove(&name).expect("resolved binding"),
        );
    }
    Ok((vars.into_iter().collect(), sources, maskable, warnings))
}

impl Config {
    /// Hydrate already-collected `rules` against an already-resolved
    /// `saved_user`. Hot-path callers (`run_inject`/`handle_set`) should use
    /// this directly with the single `collect`+`saved_user` resolved at the
    /// top of the invocation, instead of [`Config::collect_hydrate`] which
    /// re-resolves both.
    pub async fn hydrate_rules(
        &self,
        rules: &[(PathBuf, LadeRule)],
        saved_user: &Option<String>,
    ) -> Result<(
        HashMap<Output, HashMap<String, String>>,
        HashMap<String, String>,
        FxHashSet<String>,
        Vec<String>,
    )> {
        let bindings = bindings_from_rules(rules, saved_user).await?;
        hydrate_bindings(bindings).await
    }

    /// Hydrate ticket secrets from a pre-event. `op_sa` is the resolved
    /// `1password_service_account` URI, not the token.
    pub async fn hydrate_work(
        secrets: &[TicketSecret],
        op_sa: Option<&str>,
    ) -> Result<(
        HashMap<Output, HashMap<String, String>>,
        HashMap<String, String>,
        FxHashSet<String>,
        Vec<String>,
    )> {
        let bindings = bindings_from_ticket(secrets, op_sa).await?;
        hydrate_bindings(bindings).await
    }

    #[cfg(test)]
    pub async fn collect_hydrate(
        &self,
        command: &str,
    ) -> Result<(
        HashMap<Output, HashMap<String, String>>,
        HashMap<String, String>,
        FxHashSet<String>,
        Vec<String>,
    )> {
        let saved_user = super::saved_user().await?;
        self.hydrate_rules(&self.collect(command), &saved_user)
            .await
    }
}
