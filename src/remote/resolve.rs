//! The laptop's own resolution of a remote's walk: what `lade connect`
//! resolves and caches, and what `__remote` serves and resolves again once
//! a value is past its ttl.

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex};

use crate::config::{LadeFile, LadeRule, LadeSecret, ResolvedEntry, body_entries};

use super::files::WalkFile;

/// One rule body of the walk: the cache row the laptop keeps for it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Body {
    pub file: usize,
    pub pattern: String,
    pub index: usize,
}

/// A provider binding whose ref is a template: the remote renders it, and
/// the laptop resolves what it renders when the template yields it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Templated {
    pub at: usize,
    pub key: String,
    pub name: String,
    pub source: String,
}

/// A walk `lade connect` resolved on the laptop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ResolvedWalk {
    /// As passed to `lade connect`.
    pub path: String,
    /// The remote directory the walk starts from.
    pub dir: String,
    pub user: Option<String>,
    /// `human` or `agent`: who ran `lade connect`.
    pub by: String,
    pub files: Vec<WalkFile>,
    pub bodies: Vec<Body>,
    /// Each provider ref the laptop's resolution yields, as the remote
    /// renders it, with its body and binding name.
    pub refs: BTreeMap<String, (usize, String)>,
    pub tunnels: BTreeSet<String>,
    pub templates: Vec<Templated>,
    /// Templated provider bindings the laptop cannot render at connect, with
    /// what they need: they resolve when the remote asks for them.
    pub deferred: Vec<String>,
}

/// What resolves on the laptop: the schemes the remote sends up the link.
/// Raw values stay so templates render as they do on the remote.
fn goes_up(value: &str) -> bool {
    match value.split_once("://") {
        Some((scheme, _)) => lade_sdk::upstream::SCHEMES.contains(&scheme),
        None => true,
    }
}

fn is_upstream(value: &str) -> bool {
    value
        .split_once("://")
        .is_some_and(|(scheme, _)| lade_sdk::upstream::SCHEMES.contains(&scheme))
}

fn parse(file: &WalkFile) -> Result<LadeFile> {
    LadeFile::from_text(&file.text, Path::new(&file.path))
        .with_context(|| format!("failed to parse {}", file.path))
}

/// The provider bindings of `rule` whose ref is a template.
fn templates(rule: &LadeRule, user: &Option<String>, at: usize) -> Result<Vec<Templated>> {
    let mut templated = Vec::new();
    for entry in body_entries(rule, user) {
        if let ResolvedEntry::Secret { key, value } = entry
            && is_upstream(&value)
            && lade_sdk::Template::parse(&value)
                .dependencies()
                .next()
                .is_some()
        {
            let (name, _) = crate::config::binding_name(&key)?;
            templated.push(Templated {
                at,
                key,
                name,
                source: value,
            });
        }
    }
    Ok(templated)
}

fn rule<'a>(parsed: &'a [LadeFile], body: &Body) -> Result<&'a LadeRule> {
    parsed
        .get(body.file)
        .and_then(|file| file.commands.get(&body.pattern))
        .and_then(|rules| rules.get(body.index))
        .with_context(|| format!("no rule \"{}\" in the resolved walk", body.pattern))
}

/// The body with only what resolves on the laptop: `file://` and shell
/// sources are the remote's own. A binding whose template needs a value
/// only the remote has, or another body's, stays out until the remote asks
/// for it, named in the second half with what it needs.
fn laptop_part(rule: &LadeRule, user: &Option<String>) -> Result<(Option<LadeRule>, Vec<String>)> {
    let mut keep = BTreeMap::new();
    for entry in body_entries(rule, user) {
        if let ResolvedEntry::Secret { key, value } = entry
            && goes_up(&value)
        {
            let (name, _) = crate::config::binding_name(&key)?;
            keep.insert(key, (name, value));
        }
    }
    let mut deferred = Vec::new();
    loop {
        let names = keep
            .values()
            .map(|(name, _)| name.clone())
            .collect::<BTreeSet<_>>();
        let missing = keep.iter().find_map(|(key, (name, value))| {
            lade_sdk::Template::parse(value)
                .dependencies()
                .find(|dependency| !names.contains(*dependency))
                .map(|dependency| {
                    (
                        key.clone(),
                        name.clone(),
                        value.clone(),
                        dependency.to_string(),
                    )
                })
        });
        let Some((key, name, value, dependency)) = missing else {
            break;
        };
        keep.remove(&key);
        if is_upstream(&value) {
            deferred.push(format!(
                "{name} needs {dependency}: it resolves when the remote asks for it"
            ));
        }
    }
    if !keep.values().any(|(_, value)| is_upstream(value)) {
        return Ok((None, deferred));
    }
    let mut part = rule.clone();
    part.secrets.retain(|key, _| keep.contains_key(key));
    // A `.file` output is the remote's to write: here only values count.
    if let Some(config) = part.config.as_mut() {
        config.file = None;
    }
    Ok((Some(part), deferred))
}

/// What the laptop resolved of one body.
pub(crate) struct Resolved {
    /// Provider values by binding name, private ones included.
    pub values: HashMap<String, String>,
    /// Each provider ref as the remote renders it, with its binding name.
    pub refs: Vec<(String, String)>,
    pub deferred: Vec<String>,
}

pub(crate) async fn resolve_body(rule: &LadeRule, user: &Option<String>) -> Result<Resolved> {
    let (part, deferred) = laptop_part(rule, user)?;
    let Some(part) = part else {
        return Ok(Resolved {
            values: HashMap::new(),
            refs: Vec::new(),
            deferred,
        });
    };
    let home = crate::home_dir().context("the laptop's Lade finds no home directory")?;
    let (vars, sources, _, _, private) =
        crate::config::hydrate_bodies(&[(home, part)], user).await?;
    let all = vars
        .into_values()
        .flatten()
        .chain(private)
        .collect::<HashMap<_, _>>();
    let mut values = HashMap::new();
    let mut refs = Vec::new();
    // `sources` holds each ref as rendered for its provider.
    for (name, source) in sources {
        if !is_upstream(&source) {
            continue;
        }
        if let Some(value) = all.get(&name) {
            values.insert(name.clone(), value.clone());
        }
        refs.push((source, name));
    }
    Ok(Resolved {
        values,
        refs,
        deferred,
    })
}

/// Every body of the walk resolved on the laptop, its values put in the
/// hub's cache. Returns the walk and what could not resolve, by rule.
pub(crate) async fn resolve_walk(
    path: &str,
    dir: &str,
    files: Vec<WalkFile>,
    user: Option<String>,
    by: &str,
    walk: [u8; 32],
) -> Result<(ResolvedWalk, Vec<String>)> {
    let parsed = files.iter().map(parse).collect::<Result<Vec<_>>>()?;
    let mut resolved = ResolvedWalk {
        path: path.to_string(),
        dir: dir.to_string(),
        user: user.clone(),
        by: by.to_string(),
        files,
        bodies: Vec::new(),
        refs: BTreeMap::new(),
        tunnels: BTreeSet::new(),
        templates: Vec::new(),
        deferred: Vec::new(),
    };
    let mut failed = Vec::new();
    for (file, lade) in parsed.iter().enumerate() {
        for (pattern, rules) in &lade.commands {
            for (index, rule) in rules.iter().enumerate() {
                for entry in body_entries(rule, &user) {
                    if let ResolvedEntry::Tunnel { uri, .. } = entry
                        && !uri.starts_with("kubefwd://")
                    {
                        resolved.tunnels.insert(uri);
                    }
                }
                let body = Body {
                    file,
                    pattern: pattern.clone(),
                    index,
                };
                match resolve_body(rule, &user).await {
                    Ok(part) => {
                        let at = resolved.bodies.len();
                        resolved.templates.extend(templates(rule, &user, at)?);
                        for (rendered, name) in part.refs {
                            resolved.refs.insert(rendered, (at, name));
                        }
                        resolved.deferred.extend(
                            part.deferred
                                .into_iter()
                                .map(|why| format!("\"{pattern}\": {why}")),
                        );
                        put(&resolved, (&body, None), rule, walk, &part.values);
                        resolved.bodies.push(body);
                    }
                    Err(e) => failed.push(format!("\"{pattern}\": {e:#}")),
                }
            }
        }
    }
    Ok((resolved, failed))
}

type BodyLocks = Mutex<HashMap<([u8; 32], usize), Arc<tokio::sync::Mutex<()>>>>;

/// One resolution of a body at a time: concurrent requests for it wait and
/// read the cache, instead of each prompting on the laptop.
fn body_lock(walk: [u8; 32], at: usize) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: LazyLock<BodyLocks> = LazyLock::new(Mutex::default);
    Arc::clone(
        LOCKS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry((walk, at))
            .or_default(),
    )
}

/// The values of `names` in `body`, from the hub's cache, or resolved again
/// on the laptop and cached.
pub(crate) async fn values(
    resolved: &ResolvedWalk,
    at: usize,
    walk: [u8; 32],
    names: &BTreeSet<String>,
) -> Result<HashMap<String, String>> {
    let body = resolved
        .bodies
        .get(at)
        .context("no such body in the resolved walk")?;
    let lock = body_lock(walk, at);
    let _held = lock.lock().await;
    if let Some(hit) = get(resolved, body, walk, None)
        && names.iter().all(|name| hit.contains_key(name))
    {
        return Ok(hit);
    }
    let parsed = resolved
        .files
        .iter()
        .map(parse)
        .collect::<Result<Vec<_>>>()?;
    let rule = rule(&parsed, body)?;
    let values = resolve_body(rule, &resolved.user).await?.values;
    put(resolved, (body, None), rule, walk, &values);
    if let Some(missing) = names.iter().find(|name| !values.contains_key(*name)) {
        bail!(
            "the laptop's resolution of \"{}\" yields no {missing}",
            body.pattern
        );
    }
    Ok(values)
}

/// The templated binding of the walk that yields `rendered`: the last one,
/// as a later rule overrides an earlier one locally.
pub(crate) fn template_for<'a>(
    resolved: &'a ResolvedWalk,
    rendered: &str,
) -> Option<&'a Templated> {
    resolved
        .templates
        .iter()
        .rev()
        .find(|templated| lade_sdk::Template::parse(&templated.source).yields(rendered))
}

/// The value of `rendered`, a ref the remote rendered from `templated`,
/// from the hub's cache, or resolved on the laptop with its body's config
/// and cached apart from the body's other values.
pub(crate) async fn templated_value(
    resolved: &ResolvedWalk,
    templated: &Templated,
    rendered: &str,
    walk: [u8; 32],
) -> Result<String> {
    let body = resolved
        .bodies
        .get(templated.at)
        .context("no such body in the resolved walk")?;
    let lock = body_lock(walk, templated.at);
    let _held = lock.lock().await;
    if let Some(value) =
        get(resolved, body, walk, Some(rendered)).and_then(|mut hit| hit.remove(&templated.name))
    {
        return Ok(value);
    }
    let parsed = resolved
        .files
        .iter()
        .map(parse)
        .collect::<Result<Vec<_>>>()?;
    let rule = rule(&parsed, body)?;
    let mut part = rule.clone();
    part.secrets = HashMap::from([(
        templated.key.clone(),
        LadeSecret::Secret(rendered.to_string()),
    )]);
    let values = resolve_body(&part, &resolved.user).await?.values;
    put(resolved, (body, Some(rendered)), rule, walk, &values);
    values.get(&templated.name).cloned().with_context(|| {
        format!(
            "the laptop's resolution of \"{}\" yields no {}",
            body.pattern, templated.name
        )
    })
}

/// `rendered`: a templated ref as the remote rendered it, cached apart.
fn key(
    resolved: &ResolvedWalk,
    body: &Body,
    walk: [u8; 32],
    rendered: Option<&str>,
) -> crate::hub::RemoteBody {
    crate::hub::RemoteBody {
        cwd: resolved.dir.clone(),
        path: resolved.files[body.file].path.clone(),
        rule: body.pattern.clone(),
        when: match rendered {
            Some(rendered) => format!("remote#{}#{rendered}", body.index),
            None => format!("remote#{}", body.index),
        },
        walk,
        user: resolved.user.clone().unwrap_or_default(),
    }
}

fn get(
    resolved: &ResolvedWalk,
    body: &Body,
    walk: [u8; 32],
    rendered: Option<&str>,
) -> Option<HashMap<String, String>> {
    crate::hub::remote_body_get(&key(resolved, body, walk, rendered))
}

fn put(
    resolved: &ResolvedWalk,
    (body, rendered): (&Body, Option<&str>),
    rule: &LadeRule,
    walk: [u8; 32],
    values: &HashMap<String, String>,
) {
    if values.is_empty() {
        return;
    }
    if let Some(ttl_ms) = crate::config::body_put_ttl_ms(rule, None) {
        crate::hub::remote_body_put(&key(resolved, body, walk, rendered), ttl_ms, values);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(text: &str) -> LadeRule {
        LadeFile::from_text(text, Path::new("/remote/lade.yml"))
            .unwrap()
            .commands["."][0]
            .clone()
    }

    #[test]
    fn only_provider_refs_and_raw_values_resolve_on_the_laptop() {
        let rule = body(
            "\".\":\n  A: op://v/i/f\n  B: file://./x\n  C: raw\n  D: sh://echo hi\n  E: kubectl://k8s:6443/ns/svc/db/5432\n",
        );
        let (part, deferred) = laptop_part(&rule, &None).unwrap();
        let mut keys = part.unwrap().secrets.keys().cloned().collect::<Vec<_>>();
        keys.sort();
        assert_eq!(keys, ["A", "C"]);
        assert!(deferred.is_empty());
    }

    #[test]
    fn a_body_without_provider_refs_has_nothing_for_the_laptop() {
        let (part, _) = laptop_part(&body("\".\":\n  B: file://./x\n  C: raw\n"), &None).unwrap();
        assert!(part.is_none());
    }

    #[test]
    fn a_ref_templated_on_a_remote_value_is_named_not_resolved() {
        let rule = body(
            "\".\":\n  VAULT: file://./vault\n  DB: op://${VAULT}/db/password\n  A: op://v/i/f\n",
        );
        let (part, deferred) = laptop_part(&rule, &None).unwrap();
        let keys = part.unwrap().secrets.keys().cloned().collect::<Vec<_>>();
        assert_eq!(keys, ["A"]);
        assert_eq!(deferred.len(), 1, "{deferred:?}");
        assert!(deferred[0].starts_with("DB needs VAULT"), "{deferred:?}");
    }

    #[test]
    fn the_last_template_that_yields_a_ref_serves_it_and_none_serves_another() {
        let rule = body(
            "\".\":\n  VAULT: file://./vault\n  DB: op://${VAULT}/db/password\n  A: op://v/i/f\n",
        );
        let mut templates = templates(&rule, &None, 0).unwrap();
        assert_eq!(templates.len(), 1, "{templates:?}");
        let mut later = templates[0].clone();
        later.at = 1;
        templates.push(later);
        let resolved = ResolvedWalk {
            path: "src/app".into(),
            dir: "/remote".into(),
            user: None,
            by: "human".into(),
            files: Vec::new(),
            bodies: Vec::new(),
            refs: BTreeMap::new(),
            tunnels: BTreeSet::new(),
            templates,
            deferred: Vec::new(),
        };
        let served = template_for(&resolved, "op://prod/db/password").unwrap();
        assert_eq!((served.at, served.name.as_str()), (1, "DB"));
        assert!(template_for(&resolved, "op://prod/db/username").is_none());
        assert!(template_for(&resolved, "op://v/i/f").is_none());
    }
}
