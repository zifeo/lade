use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::{
    LadeRule, ResolvedEntry, RuleTtl, binding_name, body_put_ttl_ms, config_in_dir, resolve_entry,
    uri_is_cacheable, when_label,
};

use super::transport::rpc;
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

/// One command's use of the hub: the bodies it may cache, with the `lade
/// ttl set` window read once. Values cross the socket in plaintext, to and
/// from a hub that checked this is its own binary; it seals them itself.
pub struct Cache {
    planned: Vec<PlannedBody>,
}

impl Cache {
    pub fn open(
        cwd: &Path,
        walk: [u8; 32],
        saved_user: &Option<String>,
        patterned: &[(PathBuf, String, LadeRule)],
    ) -> Self {
        // A hub with no memory guard caches nothing: no call, no spawn.
        if matches!(super::memory_guard(), super::MemoryGuard::Off { .. }) {
            return Self {
                planned: Vec::new(),
            };
        }
        let user = saved_user.clone().unwrap_or_default();
        let scope = cwd.to_string_lossy().into_owned();
        // A window only adds bodies: none under any window, no hub call.
        let any_window = RuleTtl::Window(chrono::Duration::hours(24));
        if plan(
            &scope,
            walk,
            &user,
            saved_user,
            patterned,
            Some(&any_window),
        )
        .is_empty()
        {
            return Self {
                planned: Vec::new(),
            };
        }
        let session = session_ttl(cwd);
        let planned = plan(&scope, walk, &user, saved_user, patterned, session.as_ref());
        Self { planned }
    }

    pub fn lookup(&self) -> CacheHit {
        let mut hit = CacheHit::empty();
        for body in &self.planned {
            let Some(values) = get_body(body) else {
                continue;
            };
            for (name, value) in values {
                if body.names.iter().any(|n| n == &name) {
                    hit.keys.insert(name.clone());
                    hit.values.insert(name, value);
                }
            }
        }
        hit
    }

    pub fn store(&self, hydrated: &HashMap<String, String>, already_cached: &HashSet<String>) {
        // A wrap served from the cache stores nothing.
        let pending = self
            .planned
            .iter()
            .filter(|body| !body.names.iter().all(|name| already_cached.contains(name)));
        for body in pending {
            let bindings: Vec<(String, Vec<u8>)> = body
                .names
                .iter()
                .filter_map(|name| Some((name.clone(), hydrated.get(name)?.as_bytes().to_vec())))
                .collect();
            if bindings.is_empty() {
                continue;
            }
            let _ = rpc(Req::Put {
                cwd: body.cwd.clone(),
                path: body.path.clone(),
                rule: body.rule.clone(),
                when: body.when.clone(),
                walk: body.walk,
                user: body.user.clone(),
                ttl_ms: body.ttl_ms,
                bindings,
            });
        }
    }
}

fn get_body(body: &PlannedBody) -> Option<Vec<(String, String)>> {
    let Rep::Hit { bindings } = rpc(Req::Get {
        cwd: body.cwd.clone(),
        path: body.path.clone(),
        rule: body.rule.clone(),
        when: body.when.clone(),
        walk: body.walk,
        user: body.user.clone(),
    })?
    else {
        return None;
    };
    bindings
        .into_iter()
        .map(|(name, value)| Some((name, String::from_utf8(value).ok()?)))
        .collect()
}

/// The `lade ttl set` window over `cwd`. Never starts a hub.
pub(crate) fn session_ttl(cwd: &Path) -> Option<RuleTtl> {
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
