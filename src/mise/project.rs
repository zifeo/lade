use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::spec::{self, Spec};
use super::walk::{walk_up, walk_up_all};

const TOML_NAMES: &[&str] = &[
    "mise.toml",
    ".mise.toml",
    "mise/config.toml",
    ".mise/config.toml",
    ".config/mise.toml",
    ".config/mise/config.toml",
];
const IGNORE_FILES: &[&str] = &[
    "mise.toml",
    ".mise.toml",
    "mise.local.toml",
    ".mise.local.toml",
    "mise/config.toml",
    ".mise/config.toml",
    ".config/mise.toml",
    ".config/mise/config.toml",
];
const IGNORE_DIRS: &[&str] = &["mise", ".mise", ".config/mise"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectTool {
    pub key: String,
    pub version: String,
}

#[derive(Deserialize)]
struct MiseToml {
    #[serde(default)]
    tools: BTreeMap<String, toml::Value>,
}

pub fn find_mise_toml(start: &Path) -> Option<PathBuf> {
    walk_up(start, |dir| {
        TOML_NAMES
            .iter()
            .map(|name| dir.join(name))
            .find(|path| path.is_file())
    })
}

pub fn ignored_config_paths(start: &Path) -> Vec<PathBuf> {
    walk_up_all(start, |dir| {
        let mut out: Vec<PathBuf> = IGNORE_FILES
            .iter()
            .map(|name| dir.join(name))
            .filter(|path| path.is_file())
            .collect();
        out.extend(
            IGNORE_DIRS
                .iter()
                .map(|name| dir.join(name))
                .filter(|path| path.is_dir()),
        );
        out
    })
}

pub fn project_tools(path: &Path) -> Vec<ProjectTool> {
    let Ok(bytes) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(parsed) = toml::from_str::<MiseToml>(&bytes) else {
        return Vec::new();
    };
    parsed
        .tools
        .into_iter()
        .filter_map(|(key, value)| version_of(&value).map(|version| ProjectTool { key, version }))
        .collect()
}

fn version_of(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(raw) => {
            if let Ok(spec) = spec::parse(raw) {
                Some(spec.version)
            } else {
                Some(raw.clone())
            }
        }
        toml::Value::Table(table) => table
            .get("version")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        _ => None,
    }
}

pub fn conflict<'a>(
    spec: &Spec,
    pin_key: &str,
    theirs: &'a [ProjectTool],
) -> Option<&'a ProjectTool> {
    theirs
        .iter()
        .find(|tool| same_tool(&tool.key, spec, pin_key) && tool.version != spec.version)
}

fn same_tool(their_key: &str, spec: &Spec, pin_key: &str) -> bool {
    their_key == pin_key
        || their_key == spec.short_name()
        || their_key == spec.backend_id()
        || their_key == spec.package
        || spec::parse(their_key)
            .ok()
            .is_some_and(|parsed| parsed.backend_id() == spec.backend_id())
}

pub fn pin_only_toml(spec: &Spec) -> String {
    compose_toml(&[], &[("_".to_string(), spec.clone())])
}

pub fn isolate_config_paths(cwd: &Path) -> Vec<PathBuf> {
    let mut out = ignored_config_paths(cwd);
    if let Some(home) = directories::UserDirs::new().map(|user| user.home_dir().to_path_buf()) {
        out.extend([
            home.join(".config/mise"),
            home.join(".config/mise.toml"),
            home.join("mise.toml"),
            home.join(".mise.toml"),
        ]);
        out.extend(ignored_config_paths(&home));
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        out.push(PathBuf::from(xdg).join("mise"));
    }
    if let Ok(dir) = std::env::var("MISE_CONFIG_DIR")
        && !dir.is_empty()
    {
        out.push(PathBuf::from(dir));
    }
    if let Ok(dir) = std::env::var("MISE_SYSTEM_CONFIG_DIR")
        && !dir.is_empty()
    {
        out.push(PathBuf::from(dir));
    }
    out.push(PathBuf::from("/etc/mise"));
    out.sort();
    out.dedup();
    out
}

pub fn compose_toml(theirs: &[ProjectTool], pins: &[(String, Spec)]) -> String {
    let mut tools: BTreeMap<String, String> = BTreeMap::new();
    for tool in theirs {
        tools.insert(tool.key.clone(), tool.version.clone());
    }
    for (key, spec) in pins {
        if let Some(existing) = tools
            .iter()
            .find(|(their_key, _)| same_tool(their_key, spec, key))
            && existing.1 == &spec.version
        {
            continue;
        }
        tools.insert(spec.backend_id(), spec.version.clone());
    }
    let mut out = String::from("[tools]\n");
    for (key, version) in tools {
        let rendered = if key
            .chars()
            .any(|ch| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
        {
            format!("\"{}\"", key.replace('"', "\\\""))
        } else {
            key
        };
        out.push_str(&format!(
            "{rendered} = \"{}\"\n",
            version.replace('"', "\\\"")
        ));
    }
    out
}

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
