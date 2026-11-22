use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::config::{
    LadeRule, ResolvedEntry, RuleTtl, binding_name, body_entries, body_put_ttl_ms, config_in_dir,
    resolve_entry, uri_is_cacheable, when_label,
};

use super::transport::{COMMAND_DEADLINE, rpc, rpc_within};
use super::wire::{Rep, Req};

/// `when` of a `lade eval --ttl` row: never a yaml `when:`.
pub(super) const EVAL_WHEN: &str = "eval";

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
/// from a hub that checked this is its own binary, and sit in its RAM in
/// plaintext until their row goes, zeroed.
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
            // § kgejwr97c.c@7s734
            return Self {
                planned: Vec::new(),
            };
        }
        let user = saved_user.clone().unwrap_or_default();
        let scope = cwd.to_string_lossy().into_owned();
        // A window only adds bodies: none under any window, no hub call.
        // § b0wwfx9g0.d@t4vnh
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
        // § end b0wwfx9g0.d
        let session = session_ttl(cwd);
        let planned = plan(&scope, walk, &user, saved_user, patterned, session.as_ref());
        Self { planned }
    }

    pub fn lookup(&self) -> CacheHit {
        let mut hit = CacheHit::empty();
        for body in &self.planned {
            let Some(values) = get_body(body) else {
                // § e9ee1e4md.b@87n3k
                continue;
            };
            for (name, value) in values {
                // § je3g2tk0d.c@m4ada
                if body.names.iter().any(|n| n == &name) {
                    hit.keys.insert(name.clone());
                    hit.values.insert(name, value);
                }
            }
        }
        hit
    }

    pub fn store(&self, hydrated: &HashMap<String, String>, already_cached: &HashSet<String>) {
        // A Put restarts the row's clock, so a body any value of which the
        // hub served is not put again: its other names are fetched on each
        // command until the row expires.
        // § hadg0brg4.a@cj944
        let pending = self
            .planned
            .iter()
            .filter(|body| !body.names.iter().any(|name| already_cached.contains(name)));
        // § end hadg0brg4.a
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

/// `lade eval --ttl`: one row per URI, cwd, and Lade user, outside any yaml
/// body. The URI is the row's rule and its key name, so `lade ttl forget
/// <uri>` drops it. A `lade ttl set` window does not apply: the flag is the ttl.
pub struct EvalCache {
    cwd: String,
    uri: String,
    user: String,
    ttl_ms: u32,
}

impl EvalCache {
    /// `None`: `ttl` is `off`, or the hub has no memory guard. A URI no
    /// rule caches (raw, tunnel, package) is an error.
    pub fn open(
        cwd: &Path,
        uri: &str,
        ttl: &RuleTtl,
        saved_user: &Option<String>,
    ) -> anyhow::Result<Option<Self>> {
        // § one y8w7xhrr1.a@m818y
        let Some(ttl_ms) = ttl.ttl_ms() else {
            return Ok(None);
        };
        // § one y8w7xhrr1.b@te74c
        if !uri_is_cacheable(uri, Some(ttl), None) {
            // § xrjs58t2q.c@vk4ht
            anyhow::bail!("--ttl: this URI is never cached");
        }
        if matches!(super::memory_guard(), super::MemoryGuard::Off { .. }) {
            // § kgejwr97c.c@xb39x
            return Ok(None);
        }
        // § y8w7xhrr1.d@89ym0
        Ok(Some(Self {
            cwd: crate::hub::scope(cwd),
            uri: uri.to_string(),
            user: saved_user.clone().unwrap_or_default(),
            ttl_ms,
        }))
    }

    fn get(&self) -> Req {
        // § y8w7xhrr1.c@csjxn
        Req::Get {
            cwd: self.cwd.clone(),
            path: String::new(),
            rule: self.uri.clone(),
            when: EVAL_WHEN.to_string(),
            walk: [0; 32],
            user: self.user.clone(),
        }
    }

    pub fn lookup(&self) -> Option<String> {
        let Rep::Hit { bindings } = rpc(self.get())? else {
            // § e9ee1e4md.b@pysza
            return None;
        };
        let (_, value) = bindings.into_iter().find(|(name, _)| name == &self.uri)?;
        String::from_utf8(value).ok()
    }

    /// Waits for a cold hub: the user asked for this value to stay.
    pub fn store(&self, value: &str) {
        // § one y8w7xhrr1.c@frdth, y8w7xhrr1.e@hn1tp
        let _ = rpc_within(
            Req::Put {
                cwd: self.cwd.clone(),
                path: String::new(),
                rule: self.uri.clone(),
                when: EVAL_WHEN.to_string(),
                walk: [0; 32],
                user: self.user.clone(),
                ttl_ms: self.ttl_ms,
                bindings: vec![(self.uri.clone(), value.as_bytes().to_vec())],
            },
            true,
            COMMAND_DEADLINE,
        );
    }
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
        // § b0wwfx9g0.c@8q913
        let Some(ttl_ms) = body_put_ttl_ms(rule, session) else {
            for name in rule_secret_names(rule, saved_user) {
                owner.remove(&name);
            }
            continue;
        };
        let path = yaml_path(dir);
        let when = when_label(rule.when()).to_string();
        let mut names = Vec::new();
        for entry in body_entries(rule, saved_user) {
            match entry {
                ResolvedEntry::Unset { key } => {
                    if let Ok((name, _)) = binding_name(&key) {
                        owner.remove(&name);
                    }
                }
                ResolvedEntry::Secret { key, value } => {
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
                ResolvedEntry::Tunnel { key, .. }
                | ResolvedEntry::Pin { key, .. }
                | ResolvedEntry::Package { key, .. } => {
                    if let Ok((name, _)) = binding_name(&key) {
                        owner.remove(&name);
                    }
                }
                ResolvedEntry::InvalidNumericSecret { .. } => {}
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
    // § je3g2tk0d.c@ctb99
    for (idx, body) in bodies.iter_mut().enumerate() {
        body.names.retain(|name| owner.get(name) == Some(&idx));
    }
    bodies.retain(|body| !body.names.is_empty());
    // § end je3g2tk0d.c
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

/// A rule body of a remote's walk the laptop resolved for `lade connect`,
/// keyed as the remote keys its own row, apart from the laptop's by `when`.
pub(crate) struct RemoteBody {
    pub cwd: String,
    pub path: String,
    pub rule: String,
    pub when: String,
    pub walk: [u8; 32],
    pub user: String,
}

pub(crate) fn remote_body_get(body: &RemoteBody) -> Option<HashMap<String, String>> {
    if matches!(super::memory_guard(), super::MemoryGuard::Off { .. }) {
        return None;
    }
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

pub(crate) fn remote_body_put(body: &RemoteBody, ttl_ms: u32, values: &HashMap<String, String>) {
    if matches!(super::memory_guard(), super::MemoryGuard::Off { .. }) {
        return;
    }
    let _ = rpc(Req::Put {
        cwd: body.cwd.clone(),
        path: body.path.clone(),
        rule: body.rule.clone(),
        when: body.when.clone(),
        walk: body.walk,
        user: body.user.clone(),
        ttl_ms,
        bindings: values
            .iter()
            .map(|(name, value)| (name.clone(), value.as_bytes().to_vec()))
            .collect(),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_ttl_does_not_opt_in_child_shell() {
        let parent = tempfile::tempdir().unwrap();
        let child = parent.path().join("child");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(
            parent.path().join("lade.yml"),
            "\"^echo\":\n  \".\":\n    ttl: 1h\n  A: op://v/i/a\n  P: \"sh://printf p\"\n",
        )
        .unwrap();
        std::fs::write(
            child.join("lade.yml"),
            "\"^echo\":\n  B: \"sh://printf b\"\n",
        )
        .unwrap();
        let config = crate::config::LadeFile::build(child.clone()).unwrap();
        let patterned = config
            .collect_for_with_pattern("echo hi", crate::config::Audience::Human)
            .unwrap();
        assert_eq!(patterned.len(), 2, "{patterned:?}");
        let scope = child.to_string_lossy().into_owned();
        let planned: Vec<String> = plan(&scope, [0; 32], "", &None, &patterned, None)
            .into_iter()
            .flat_map(|body| body.names)
            .collect();
        assert!(planned.contains(&"A".to_string()), "{planned:?}");
        assert!(planned.contains(&"P".to_string()), "{planned:?}");
        assert!(!planned.contains(&"B".to_string()), "{planned:?}");
    }
}
