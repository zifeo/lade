use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

use lade_sdk::network::is_network_scheme;

use crate::event::Event;

#[derive(Debug, Serialize)]
pub struct CommandRow {
    pub command: String,
    pub count: u64,
    pub human: u64,
    pub agent: u64,
}

#[derive(Debug, Serialize)]
pub struct RuleRow {
    pub rule: String,
    pub file: String,
    pub count: u64,
    pub human: u64,
    pub agent: u64,
    pub tags: Vec<String>,
}

pub fn group_commands(events: &[Event]) -> Vec<CommandRow> {
    let mut map: HashMap<String, CommandRow> = HashMap::new();
    for ev in events {
        let row = map.entry(ev.command.clone()).or_insert_with(|| CommandRow {
            command: ev.command.clone(),
            count: 0,
            human: 0,
            agent: 0,
        });
        bump(row, ev.audience.as_deref());
    }
    let mut out: Vec<CommandRow> = map.into_values().collect();
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.command.cmp(&b.command))
    });
    out
}

pub fn group_rules(events: &[Event]) -> Vec<RuleRow> {
    let mut map: HashMap<(String, String), RuleRow> = HashMap::new();
    for ev in events {
        let Some(items) = ev.matches.as_array() else {
            continue;
        };
        for item in items {
            let Some(rule) = item.get("rule").and_then(|r| r.as_str()) else {
                continue;
            };
            if is_catch_all(rule) {
                continue;
            }
            let file = item
                .get("file")
                .and_then(|f| f.as_str())
                .unwrap_or("")
                .to_string();
            let tags = tags_from(item.get("bindings"));
            let key = (file.clone(), rule.to_string());
            let row = map.entry(key).or_insert_with(|| RuleRow {
                rule: rule.to_string(),
                file,
                count: 0,
                human: 0,
                agent: 0,
                tags,
            });
            bump_rule(row, ev.audience.as_deref());
        }
    }
    let mut out: Vec<RuleRow> = map.into_values().collect();
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.rule.cmp(&b.rule))
    });
    out
}

fn bump(row: &mut CommandRow, audience: Option<&str>) {
    row.count += 1;
    if audience == Some("agent") {
        row.agent += 1;
    } else {
        row.human += 1;
    }
}

fn bump_rule(row: &mut RuleRow, audience: Option<&str>) {
    row.count += 1;
    if audience == Some("agent") {
        row.agent += 1;
    } else {
        row.human += 1;
    }
}

fn is_catch_all(rule: &str) -> bool {
    rule == "." || rule == ".*"
}

fn tags_from(bindings: Option<&serde_json::Value>) -> Vec<String> {
    let mut tags = Vec::new();
    let Some(items) = bindings.and_then(|b| b.as_array()) else {
        return tags;
    };
    for binding in items {
        let Some(uri) = binding.get("uri").and_then(|u| u.as_str()) else {
            continue;
        };
        let tag = uri_tag(uri);
        if !tags.iter().any(|t| t == tag) {
            tags.push(tag.to_string());
        }
    }
    tags.sort();
    tags
}

fn uri_tag(uri: &str) -> &'static str {
    let scheme = uri.split("://").next().unwrap_or("");
    if scheme == "file" {
        "file"
    } else if is_network_scheme(scheme) {
        "tunnel"
    } else {
        "env"
    }
}

pub fn git_root(cwd: &Path) -> Option<PathBuf> {
    crate::event::git_stamp(cwd).0.map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::Event;
    use serde_json::json;

    fn event(command: &str, rule: Option<(&str, &str)>, audience: &str) -> Event {
        Event {
            id: "1".into(),
            ts: "2026-01-01T00:00:00.000Z".into(),
            kind: "access".into(),
            via: Some("organic".into()),
            audience: Some(audience.into()),
            actor: None,
            repo: None,
            git_commit: None,
            command: command.into(),
            command_truncated: false,
            hydrate_ms: None,
            matches: match rule {
                Some((file, r)) => json!([{
                    "file": file,
                    "rule": r,
                    "bindings": [{ "key": "TOKEN", "uri": "op://v/i/f" }]
                }]),
                None => json!([]),
            },
            agent: None,
        }
    }

    #[test]
    fn group_commands_orders_by_count() {
        let events = vec![
            event("ls", None, "human"),
            event("ls", None, "agent"),
            event("git status", None, "human"),
        ];
        let rows = group_commands(&events);
        assert_eq!(rows[0].command, "ls");
        assert_eq!(rows[0].count, 2);
        assert_eq!(rows[0].human, 1);
        assert_eq!(rows[0].agent, 1);
        assert_eq!(rows[1].command, "git status");
        assert_eq!(rows[1].count, 1);
    }

    #[test]
    fn group_rules_skips_catch_all_and_unused() {
        let events = vec![
            event("ls", Some(("/proj", ".")), "human"),
            event(
                "npm run deploy",
                Some(("/proj", "^npm run deploy")),
                "human",
            ),
            event(
                "npm run deploy",
                Some(("/proj", "^npm run deploy")),
                "agent",
            ),
        ];
        let rows = group_rules(&events);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].rule, "^npm run deploy");
        assert_eq!(rows[0].file, "/proj");
        assert_eq!(rows[0].count, 2);
        assert_eq!(rows[0].human, 1);
        assert_eq!(rows[0].agent, 1);
        assert_eq!(rows[0].tags, vec!["env"]);
    }
}
