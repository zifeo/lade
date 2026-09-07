use std::collections::HashMap;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::{Value, json};

use crate::audience::Via;
use crate::config::{Audience, Config, LadeRule};

mod db;
mod git;
#[cfg(test)]
mod tests;
mod write;

pub use db::{info, open, prune_before, query, warmup};
pub use git::git_stamp;
#[allow(unused_imports)]
pub use write::emit;
pub use write::emit_if;
pub use write::emit_verb;

pub(crate) use db::event_from_row;

// First create + migrate holds the write lock. 250ms dropped a
// concurrent inject on CI (two_writers_do_not_corrupt).
const BUSY_MS: u64 = 5_000;

pub(crate) const EVENTS_DDL: &str = "CREATE TABLE events (
          id TEXT PRIMARY KEY,
          ts TEXT NOT NULL,
          kind TEXT NOT NULL,
          via TEXT,
          audience TEXT,
          actor TEXT,
          repo TEXT,
          git_commit TEXT,
          command TEXT NOT NULL,
          command_truncated INTEGER NOT NULL,
          hydrate_ms REAL,
          matches BLOB NOT NULL
        );
        CREATE INDEX events_ts ON events(ts);
        CREATE INDEX events_kind_ts ON events(kind, ts);
        CREATE INDEX events_repo_ts ON events(repo, ts);";

pub(crate) const EVENTS_AGENT_DDL: &str = "ALTER TABLE events ADD COLUMN agent BLOB";

pub(crate) const EVENTS_ARGV_DDL: &str = "ALTER TABLE events ADD COLUMN argv JSONB";

pub(crate) fn events_ddl() -> String {
    format!("{EVENTS_DDL}{EVENTS_AGENT_DDL};{EVENTS_ARGV_DDL};")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Seen,
    Access,
    Denied,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Seen => "seen",
            Kind::Access => "access",
            Kind::Denied => "denied",
        }
    }
}

#[derive(Debug, Serialize)]
pub struct Event {
    pub id: String,
    pub ts: String,
    pub kind: String,
    pub via: Option<String>,
    pub audience: Option<String>,
    pub actor: Option<String>,
    pub repo: Option<String>,
    pub git_commit: Option<String>,
    pub command: String,
    pub command_truncated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub argv: Option<Value>,
    pub hydrate_ms: Option<f64>,
    pub matches: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<Value>,
}

#[derive(Debug, Serialize)]
pub struct LogInfo {
    pub path: PathBuf,
    pub events: i64,
    pub bytes: u64,
}

pub fn db_path() -> PathBuf {
    // Test override, same reason as LADE_CONFIG_PATH: Windows
    // SHGetKnownFolderPath ignores HOME. Production uses ProjectDirs.
    if let Ok(p) = std::env::var("LADE_EVENTS_PATH") {
        return PathBuf::from(p);
    }
    let project = directories::ProjectDirs::from("com", "zifeo", "lade")
        .expect("cannot get directory for projet");
    project.data_local_dir().join("events.db")
}

pub fn actor(user: &Option<String>) -> Option<String> {
    user.clone()
        .or_else(crate::global_config::GlobalConfig::user_from_disk)
        .or_else(|| {
            std::env::var("USER")
                .ok()
                .or_else(|| std::env::var("USERNAME").ok())
        })
}

pub fn match_tree_from(
    rules: &[(std::path::PathBuf, String, LadeRule)],
    saved_user: &Option<String>,
) -> Value {
    let mut out = Vec::new();
    for (file, pattern, rule) in rules {
        let bindings: Vec<Value> = Config::public_bindings(rule, saved_user)
            .into_iter()
            .map(|(key, uri)| json!({ "key": key, "uri": uri }))
            .collect();
        if bindings.is_empty() {
            continue;
        }
        out.push(json!({
            "file": file.to_string_lossy(),
            "rule": pattern,
            "bindings": bindings
        }));
    }
    Value::Array(out)
}

pub fn display_line(command: &str, argv: Option<&Value>) -> Option<String> {
    if command.is_empty() {
        return None;
    }
    match argv {
        Some(Value::Array(items)) if !items.is_empty() => {
            let rest: Vec<&str> = items.iter().filter_map(Value::as_str).collect();
            if rest.is_empty() {
                Some(command.to_string())
            } else {
                Some(format!("{} {}", command, rest.join(" ")))
            }
        }
        Some(Value::Object(map)) if !map.is_empty() => {
            Some(format!("{} {}", command, Value::Object(map.clone())))
        }
        _ => Some(command.to_string()),
    }
}

pub fn logged_kind(matches: &Value) -> Kind {
    match matches.as_array() {
        Some(items) if !items.is_empty() => Kind::Access,
        _ => Kind::Seen,
    }
}

pub struct Emit {
    pub kind: Kind,
    pub via: Via,
    pub audience: Audience,
    pub actor: Option<String>,
    pub cwd: PathBuf,
    pub command: String,
    pub argv: Option<Value>,
    pub hydrated: Option<HashMap<String, String>>,
    pub matches: Value,
    pub hydrate_ms: Option<f64>,
    pub agent: Option<Value>,
}
