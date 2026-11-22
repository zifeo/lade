use anyhow::{Result, bail};
use futures::stream::{FuturesUnordered, StreamExt};
use lade_sdk::{Dag, Template, hydrate_one, hydrate_with_maskable};
use rustc_hash::{FxHashMap, FxHashSet};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::PathBuf,
};

use super::resolve::{ResolvedEntry, binding_name, body_entries, check_env_name, replacing_name};
use super::secret::resolve_lade_secret;
use super::{Config, LadeRule, Output, is_shell_uri};
use crate::ticket::TicketSecret;

#[derive(Debug, Clone)]
struct Binding {
    private: bool,
    source: String,
    cwd: PathBuf,
    output: Output,
    extra_env: HashMap<String, String>,
}

fn routed_upstream(uri: &str) -> bool {
    uri.split_once("://")
        .is_some_and(|(scheme, _)| lade_sdk::upstream::routes(scheme))
}

async fn bindings_from_rules(
    rules: &[(PathBuf, LadeRule)],
    saved_user: &Option<String>,
) -> Result<HashMap<String, Binding>> {
    let mut bindings = HashMap::<String, Binding>::new();
    for (cwd, rule) in rules {
        let output = rule.config.as_ref().and_then(|config| config.file.clone());
        // § zk7sjvann.a@j6bxs
        let extra_env = if let Some(uri) = rule
            .config
            .as_ref()
            .and_then(|config| config.onepassword_service_account.as_ref())
            .and_then(|secret| resolve_lade_secret(secret, saved_user))
            // On an attached remote the laptop resolves the body with its
            // config: the token stays there.
            .filter(|uri| !routed_upstream(uri))
        {
            HashMap::from([(
                "OP_SERVICE_ACCOUNT_TOKEN".to_string(),
                hydrate_one(uri, cwd, &HashMap::new()).await?,
            )])
        } else {
            HashMap::new()
        };
        for entry in body_entries(rule, saved_user) {
            match entry {
                ResolvedEntry::Unset { key } => {
                    bindings.remove(&replacing_name(&key, true)?);
                }
                ResolvedEntry::Tunnel { key, .. }
                | ResolvedEntry::Pin { key, .. }
                | ResolvedEntry::Package { key, .. } => {
                    bindings.remove(&replacing_name(&key, false)?);
                }
                ResolvedEntry::InvalidNumericSecret { key } => bail!(
                    "numeric key '{}' must use a tunnel URI (kubectl://, kubefwd://, tsh://)",
                    key
                ),
                ResolvedEntry::Secret { key, value } => {
                    let (name, private) = binding_name(&key)?;
                    check_env_name(&name, output.is_some())?;
                    let binding = Binding {
                        private,
                        source: value,
                        cwd: cwd.clone(),
                        output: output.as_ref().map(|path| cwd.join(path)),
                        extra_env: extra_env.clone(),
                    };
                    bindings.insert(name, binding);
                }
            }
        }
    }
    Ok(bindings)
}

async fn bindings_from_ticket(
    secrets: &[TicketSecret],
    seed: &HashMap<String, String>,
) -> Result<HashMap<String, Binding>> {
    let mut op_tokens = HashMap::<(PathBuf, String), String>::new();
    let mut bindings = HashMap::<String, Binding>::new();
    for secret in secrets {
        // § one zk7sjvann.b@awfxj, zk7sjvann.a@d7p4v
        let extra_env = if seed.contains_key(&secret.key) {
            HashMap::new()
        } else if let Some(uri) = secret.op_sa.as_deref() {
            let key = (secret.cwd.clone(), uri.to_string());
            let token = if let Some(token) = op_tokens.get(&key) {
                token.clone()
            } else {
                let token = hydrate_one(uri.to_string(), &secret.cwd, &HashMap::new()).await?;
                op_tokens.insert(key, token.clone());
                token
            };
            HashMap::from([("OP_SERVICE_ACCOUNT_TOKEN".to_string(), token)])
        } else {
            HashMap::new()
        };
        if let Some(existing) = bindings.get(&secret.key)
            && existing.private != secret.private
        {
            // § dyat01nyj.a@7mvtr
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

/// Exported values by output, name to config source, maskable names and
/// sources, warnings, and the values of private bindings by name.
pub(crate) type Hydrated = (
    HashMap<Output, HashMap<String, String>>,
    HashMap<String, String>,
    FxHashSet<String>,
    Vec<String>,
    HashMap<String, String>,
);

/// `seed` holds values the hub served: those bindings stay in the graph as
/// resolved nodes, so a reference to one resolves like a fetched one.
async fn hydrate_bindings(
    bindings: HashMap<String, Binding>,
    seed: &HashMap<String, String>,
    mut on_group: impl FnMut(&HashSet<String>),
) -> Result<Hydrated> {
    // § v3rjmav3z@dq35b
    let templates = bindings
        .iter()
        .map(|(name, binding)| (name.clone(), Template::parse(&binding.source)))
        .collect::<HashMap<_, _>>();
    let dag = Dag::new(templates)?;
    let mut degrees = dag.indegrees();
    let mut values = HashMap::<String, String>::new();
    let mut sources = HashMap::<String, String>::new();
    let mut maskable = FxHashSet::default();
    let mut warnings = Vec::new();
    let mut ready = Vec::new();
    // § v3rjmav3z.a@2dh50
    let served = |name: &str| bindings.contains_key(name) && seed.contains_key(name);
    for name in dag.initial_ready() {
        if !served(&name) {
            ready.push(name);
        }
    }
    for (name, value) in seed {
        let Some(binding) = bindings.get(name) else {
            continue;
        };
        values.insert(name.clone(), value.clone());
        sources.insert(name.clone(), binding.source.clone());
        // § qq076qjq2.a@sysb8
        maskable.insert(name.clone());
        for dependent in dag.dependents(name) {
            let degree = degrees.get_mut(dependent).expect("planned dependent");
            *degree -= 1;
            if *degree == 0 && !served(dependent) {
                ready.push(dependent.clone());
            }
        }
    }
    ready.sort();
    // § end v3rjmav3z.a

    let mut running = FuturesUnordered::new();
    while !ready.is_empty() || !running.is_empty() {
        let batch = std::mem::take(&mut ready);
        let mut groups =
            BTreeMap::<(PathBuf, Vec<(String, String)>, bool), HashMap<String, String>>::new();
        for name in &batch {
            // § v3rjmav3z.c@ttz6h
            let binding = bindings.get(name).expect("planned binding");
            let template = dag.template(name).expect("planned template");
            let shell_source = is_shell_uri(&binding.source);
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
        // The first group that fails ends the hydrate with nothing returned.
        // § d5fjsqxna.a@p1ydh
        let (configured, (resolved, group_maskable, group_warnings)) = running
            .next()
            .await
            .expect("a planned DAG must have an active group")?;
        on_group(&configured.keys().cloned().collect());
        for (name, value) in resolved {
            // § qq076qjq2.a@t44z7
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
                if *degree == 0 && !served(dependent) {
                    newly_ready.insert(dependent.clone());
                }
            }
        }
        ready.extend(newly_ready);
    }
    // § end v3rjmav3z

    let mut vars = FxHashMap::<Output, HashMap<String, String>>::default();
    let mut private = HashMap::new();
    for (name, binding) in bindings {
        let value = values.remove(&name).expect("resolved binding");
        // § dyat01nyj@fsv9h
        if binding.private {
            // § qq076qjq2.b@e91vt
            private.insert(name, value);
            continue;
        }
        vars.entry(binding.output).or_default().insert(name, value);
    }
    Ok((
        vars.into_iter().collect(),
        sources,
        maskable,
        warnings,
        private,
    ))
}

/// Hydrate `rules` with no `Config`: a walk read from text, the remote's
/// on the laptop.
pub(crate) async fn hydrate_bodies(
    rules: &[(PathBuf, LadeRule)],
    saved_user: &Option<String>,
) -> Result<Hydrated> {
    // § w8e03rv88.c@8199k
    // Plans first, so a binding or reference error fails before a
    // service-account token is fetched.
    super::plan::secrets_from_rules(rules, saved_user)?;
    let bindings = bindings_from_rules(rules, saved_user).await?;
    hydrate_bindings(bindings, &HashMap::new(), |_| {}).await
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
        let (vars, sources, maskable, warnings, _private) =
            hydrate_bodies(rules, saved_user).await?;
        Ok((vars, sources, maskable, warnings))
    }

    /// Hydrate ticket secrets from a pre-event. Each secret carries its own
    /// rule's `1password_service_account` URI. `seed` holds the values the
    /// hub served.
    pub async fn hydrate_work_on_group(
        secrets: &[TicketSecret],
        seed: &HashMap<String, String>,
        on_group: impl FnMut(&HashSet<String>),
    ) -> Result<Hydrated> {
        let bindings = bindings_from_ticket(secrets, seed).await?;
        hydrate_bindings(bindings, seed, on_group).await
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
        self.hydrate_rules(&self.collect(command)?, &saved_user)
            .await
    }
}
