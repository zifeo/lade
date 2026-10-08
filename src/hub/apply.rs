use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::{
    LadeRule, ResolvedEntry, RuleTtl, binding_name, body_put_ttl_ms, config_in_dir, resolve_entry,
    uri_is_cacheable, when_label,
};

use super::client::{daemon_off, rpc};
use super::crypto::{aad, open, seal, zero_key};
use super::key::wrap_key;
use super::wire::{Rep, Req};

pub struct CacheHit {
    pub values: HashMap<String, String>,
    pub keys: HashSet<String>,
}

impl CacheHit {
    pub fn empty() -> Self {
        Self {
            values: HashMap::new(),
            keys: HashSet::new(),
        }
    }
}

struct PlannedBody {
    cwd: String,
    path: String,
    rule: String,
    when: String,
    walk: [u8; 32],
    user: String,
    ttl_ms: u32,
    names: Vec<String>,
}

pub fn lookup(
    cwd: &Path,
    walk: [u8; 32],
    saved_user: &Option<String>,
    patterned: &[(PathBuf, String, LadeRule)],
) -> CacheHit {
    let mut hit = CacheHit {
        values: HashMap::new(),
        keys: HashSet::new(),
    };
    if daemon_off() {
        return hit;
    }
    let user = saved_user.clone().unwrap_or_default();
    let session = session_for(cwd);
    let cwd = cwd.to_string_lossy().into_owned();
    let planned = plan(&cwd, walk, &user, saved_user, patterned, session.as_ref());
    if planned.is_empty() {
        return hit;
    }
    let Some(mut key) = wrap_key() else {
        return hit;
    };
    for body in planned {
        let Some(values) = get_body(&mut key, &body) else {
            continue;
        };
        for (name, value) in values {
            if body.names.iter().any(|n| n == &name) {
                hit.keys.insert(name.clone());
                hit.values.insert(name, value);
            }
        }
    }
    zero_key(&mut key);
    hit
}

pub fn store(
    cwd: &Path,
    walk: [u8; 32],
    saved_user: &Option<String>,
    patterned: &[(PathBuf, String, LadeRule)],
    hydrated: &HashMap<String, String>,
    already_cached: &HashSet<String>,
) {
    if daemon_off() {
        return;
    }
    let user = saved_user.clone().unwrap_or_default();
    let session = session_for(cwd);
    let cwd = cwd.to_string_lossy().into_owned();
    let planned = plan(&cwd, walk, &user, saved_user, patterned, session.as_ref());
    if planned.is_empty() {
        return;
    }
    let Some(mut key) = wrap_key() else {
        return;
    };
    for body in planned {
        if body.names.iter().all(|name| already_cached.contains(name)) {
            continue;
        }
        let aad = aad(
            &body.cwd, &body.path, &body.rule, &body.when, &body.walk, &body.user,
        );
        let mut bindings = Vec::new();
        for name in &body.names {
            let Some(value) = hydrated.get(name) else {
                continue;
            };
            let Some(blob) = seal(&key, &aad, value.as_bytes()) else {
                bindings.clear();
                break;
            };
            bindings.push((name.clone(), blob));
        }
        if bindings.is_empty() {
            continue;
        }
        let _ = rpc(Req::Put {
            cwd: body.cwd,
            path: body.path,
            rule: body.rule,
            when: body.when,
            walk: body.walk,
            user: body.user,
            ttl_ms: body.ttl_ms,
            bindings,
        });
    }
    zero_key(&mut key);
}

fn get_body(key: &mut [u8; 32], body: &PlannedBody) -> Option<Vec<(String, String)>> {
    let rep = rpc(Req::Get {
        cwd: body.cwd.clone(),
        path: body.path.clone(),
        rule: body.rule.clone(),
        when: body.when.clone(),
        walk: body.walk,
        user: body.user.clone(),
    })?;
    let Rep::Hit { bindings } = rep else {
        return None;
    };
    let aad = aad(
        &body.cwd, &body.path, &body.rule, &body.when, &body.walk, &body.user,
    );
    let mut out = Vec::new();
    for (name, blob) in bindings {
        let plain = open(key, &aad, &blob)?;
        let value = String::from_utf8(plain).ok()?;
        out.push((name, value));
    }
    Some(out)
}

fn session_for(cwd: &Path) -> Option<RuleTtl> {
    let raw = crate::hub::window(&crate::hub::scope(cwd))?;
    RuleTtl::parse(&raw).ok()
}

fn plan(
    cwd: &str,
    walk: [u8; 32],
    user: &str,
    saved_user: &Option<String>,
    patterned: &[(PathBuf, String, LadeRule)],
    session: Option<&RuleTtl>,
) -> Vec<PlannedBody> {
    let mut owner: HashMap<String, usize> = HashMap::new();
    let mut bodies = Vec::new();
    for (dir, rule_pattern, rule) in patterned {
        let Some(ttl_ms) = body_put_ttl_ms(rule, session) else {
            for name in rule_secret_names(rule, saved_user) {
                owner.remove(&name);
            }
            continue;
        };
        let path = yaml_path(dir);
        let when = when_label(rule.when()).to_string();
        let mut names = Vec::new();
        for (key, secret) in &rule.secrets {
            match resolve_entry(key, secret, saved_user) {
                Some(ResolvedEntry::Unset { key }) => {
                    if let Ok((name, _)) = binding_name(&key) {
                        owner.remove(&name);
                    }
                }
                Some(ResolvedEntry::Secret { key, value }) => {
                    let Ok((name, _)) = binding_name(&key) else {
                        continue;
                    };
                    if uri_is_cacheable(&value, rule.ttl(), session) {
                        names.push(name.clone());
                        owner.insert(name, bodies.len());
                    } else {
                        owner.remove(&name);
                    }
                }
                Some(
                    ResolvedEntry::Tunnel { key, .. }
                    | ResolvedEntry::Pin { key, .. }
                    | ResolvedEntry::Package { key, .. },
                ) => {
                    if let Ok((name, _)) = binding_name(&key) {
                        owner.remove(&name);
                    }
                }
                _ => {}
            }
        }
        bodies.push(PlannedBody {
            cwd: cwd.to_string(),
            path,
            rule: rule_pattern.clone(),
            when,
            walk,
            user: user.to_string(),
            ttl_ms,
            names,
        });
    }
    for (idx, body) in bodies.iter_mut().enumerate() {
        body.names.retain(|name| owner.get(name) == Some(&idx));
    }
    bodies.retain(|body| !body.names.is_empty());
    bodies
}

fn rule_secret_names(rule: &LadeRule, saved_user: &Option<String>) -> Vec<String> {
    let mut names = Vec::new();
    for (key, secret) in &rule.secrets {
        if let Some(ResolvedEntry::Secret { key, .. }) = resolve_entry(key, secret, saved_user)
            && let Ok((name, _)) = binding_name(&key)
        {
            names.push(name);
        }
    }
    names
}

fn yaml_path(dir: &Path) -> String {
    config_in_dir(dir)
        .ok()
        .flatten()
        .unwrap_or_else(|| dir.to_path_buf())
        .to_string_lossy()
        .into_owned()
}
