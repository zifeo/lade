mod heuristics;

#[cfg(test)]
mod tests;

use std::collections::HashMap;

use serde_json::Value;

use crate::pretool::split_command_env_prefix;

use heuristics::{apply_context_needles, apply_prefix_and_length, replace_known_values};

const CAP: usize = 1024;

pub struct Redacted {
    pub command: String,
}

/// Redact a command for the diary. `hydrated` is every public value on
/// `access`. `denied` and `seen` pass `None`.
pub fn redact_command(raw: &str, hydrated: Option<&HashMap<String, String>>) -> Redacted {
    let (_, mut command) = split_command_env_prefix(raw);
    if let Some(values) = hydrated {
        command = replace_known_values(&command, values);
        if values
            .values()
            .any(|value| !value.is_empty() && command.contains(value.as_str()))
        {
            return Redacted {
                command: String::new(),
            };
        }
    }
    command = apply_context_needles(&command);
    command = apply_prefix_and_length(&command);
    Redacted { command }
}

pub fn peel_command(line: &str) -> (String, Option<Value>) {
    let tokens = tokenize(line);
    match tokens.as_slice() {
        [] => (String::new(), None),
        [command] => (command.clone(), None),
        [command, rest @ ..] => (
            command.clone(),
            Some(Value::Array(
                rest.iter().cloned().map(Value::String).collect(),
            )),
        ),
    }
}

pub fn cap_text(text: String) -> (String, bool) {
    let truncated = text.chars().count() > CAP;
    if truncated {
        (text.chars().take(CAP).collect(), true)
    } else {
        (text, false)
    }
}

fn tokenize(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote = None;
    for ch in line.chars() {
        match (quote, ch) {
            (None, '\'') | (None, '"') => quote = Some(ch),
            (Some(q), c) if c == q => quote = None,
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            (_, c) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn redact_argv(
    value: Option<Value>,
    hydrated: Option<&HashMap<String, String>>,
) -> Option<Value> {
    let mut value = value?;
    if value.is_null() {
        return None;
    }
    if let Some(values) = hydrated {
        replace_known_in_json(&mut value, values);
        if json_contains_hydrate(&value, values) {
            return None;
        }
    }
    walk_scrub(&mut value);
    cap_json(&mut value);
    match &value {
        Value::Null => None,
        Value::Object(map) if map.is_empty() => None,
        Value::Array(items) if items.is_empty() => None,
        _ => Some(value),
    }
}

fn replace_known_in_json(value: &mut Value, hydrated: &HashMap<String, String>) {
    match value {
        Value::String(text) => *text = replace_known_values(text, hydrated),
        Value::Array(items) => {
            for item in items {
                replace_known_in_json(item, hydrated);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                replace_known_in_json(item, hydrated);
            }
        }
        _ => {}
    }
}

fn json_contains_hydrate(value: &Value, hydrated: &HashMap<String, String>) -> bool {
    match value {
        Value::String(text) => hydrated
            .values()
            .any(|secret| !secret.is_empty() && text.contains(secret.as_str())),
        Value::Array(items) => items
            .iter()
            .any(|item| json_contains_hydrate(item, hydrated)),
        Value::Object(map) => map
            .values()
            .any(|item| json_contains_hydrate(item, hydrated)),
        _ => false,
    }
}

fn walk_scrub(value: &mut Value) {
    match value {
        Value::String(text) => {
            *text = apply_context_needles(text);
            *text = apply_prefix_and_length(text);
        }
        Value::Array(items) => {
            for item in items {
                walk_scrub(item);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                walk_scrub(item);
            }
        }
        _ => {}
    }
}

fn cap_json(value: &mut Value) {
    match value {
        Value::String(text) if text.chars().count() > CAP => {
            *text = text.chars().take(CAP).collect();
        }
        Value::Array(items) => {
            for item in items {
                cap_json(item);
            }
        }
        Value::Object(map) => {
            for item in map.values_mut() {
                cap_json(item);
            }
        }
        _ => {}
    }
}
