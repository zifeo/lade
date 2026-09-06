use std::collections::HashMap;
use std::io::Cursor;

use serde_json::Value;

use crate::pretool::split_command_env_prefix;
use crate::redact::Redactor;

const CAP: usize = 1024;

const CONTEXT_NEEDLES: &[&[u8]] = &[
    b"authorization:",
    b"bearer ",
    b"x-api-key:",
    b"x-apikey:",
    b"api-key:",
    b"access_token=",
    b"api_key=",
    b"apikey=",
];

const AUTH_SCHEMES: &[&[u8]] = &[b"bearer ", b"token ", b"basic "];

const PREFIXES: &[&str] = &[
    "AKIA", "sk-", "sk_live_", "ghp_", "gho_", "xox", "eyJ", "glpat-",
];

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

fn replace_known_values(command: &str, values: &HashMap<String, String>) -> String {
    let Some(redactor) = Redactor::new(values, "${{}}") else {
        return command.to_string();
    };
    let mut out = Vec::new();
    if redactor
        .stream(Cursor::new(command.as_bytes()), &mut out)
        .is_err()
    {
        return command.to_string();
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn apply_context_needles(command: &str) -> String {
    let bytes = command.as_bytes();
    let lower: Vec<u8> = bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut spans = Vec::new();
    for needle in CONTEXT_NEEDLES {
        let mut from = 0;
        while let Some(rel) = find_sub(&lower[from..], needle) {
            let after = from + rel + needle.len();
            let skip_schemes = *needle == b"authorization:";
            if let Some(span) = next_cred(&lower, after, skip_schemes) {
                spans.push(span);
            }
            from = after;
            if from >= lower.len() {
                break;
            }
        }
    }
    if spans.is_empty() {
        return command.to_string();
    }
    spans.sort_by_key(|&(start, _)| start);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for span in spans {
        if let Some(last) = merged.last_mut()
            && span.0 < last.1
        {
            last.1 = last.1.max(span.1);
            continue;
        }
        merged.push(span);
    }
    let mut out = String::new();
    let mut pos = 0;
    for (start, end) in merged {
        if start < pos {
            continue;
        }
        out.push_str(&command[pos..start]);
        out.push('?');
        pos = end;
    }
    out.push_str(&command[pos..]);
    out
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn next_cred(hay: &[u8], after: usize, skip_schemes: bool) -> Option<(usize, usize)> {
    let mut i = after;
    while i < hay.len() && hay[i].is_ascii_whitespace() {
        i += 1;
    }
    if skip_schemes {
        for scheme in AUTH_SCHEMES {
            if hay[i..].starts_with(scheme) {
                i += scheme.len();
                break;
            }
        }
    }
    while i < hay.len() && hay[i].is_ascii_whitespace() {
        i += 1;
    }
    if i < hay.len() && hay[i] == b'$' {
        return None;
    }
    let start = i;
    while i < hay.len() {
        let b = hay[i];
        if b.is_ascii_whitespace() || matches!(b, b'"' | b'\'' | b'&' | b';') {
            break;
        }
        i += 1;
    }
    if i > start { Some((start, i)) } else { None }
}

fn apply_prefix_and_length(command: &str) -> String {
    let mut out = String::new();
    let mut rest = command;
    while let Some((tok, tail, sep)) = next_token(rest) {
        out.push_str(&sep);
        if prefix_or_len_hit(tok) {
            out.push('?');
        } else {
            out.push_str(tok);
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

fn next_token(s: &str) -> Option<(&str, &str, String)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && is_token_sep(bytes[i]) {
        i += 1;
    }
    if i == bytes.len() {
        return None;
    }
    let start = i;
    while i < bytes.len() && !is_token_sep(bytes[i]) {
        i += 1;
    }
    Some((&s[start..i], &s[i..], s[..start].to_string()))
}

fn is_token_sep(b: u8) -> bool {
    b.is_ascii_whitespace() || matches!(b, b'=' | b':' | b'"' | b'\'')
}

fn prefix_or_len_hit(tok: &str) -> bool {
    if PREFIXES.iter().any(|p| tok.starts_with(p)) {
        return true;
    }
    if tok.chars().count() < 20 {
        return false;
    }
    tok.bytes().any(|b| b.is_ascii_digit()) && tok.bytes().any(|b| b.is_ascii_alphabetic())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const SECRET: &str = "tok_example_0000000001";
    const OTHER: &str = "other_example_0000000002";

    fn seen(raw: &str) -> String {
        redact_command(raw, None).command
    }

    #[test]
    fn env_prefix_is_stripped() {
        let out = seen(&format!("TOKEN={SECRET} curl https://example.com"));
        assert_eq!(out, "curl https://example.com");
        assert!(!out.contains(SECRET));
    }

    #[test]
    fn bearer_context() {
        let out = seen(&format!(
            r#"curl -H "Authorization: Bearer {SECRET}" https://example.com"#
        ));
        assert!(out.contains("Authorization: Bearer ?"), "{out}");
        assert!(!out.contains(SECRET));
    }

    #[test]
    fn unexpanded_token_is_left() {
        let out = seen(r#"curl -H "Authorization: $TOKEN" https://example.com"#);
        assert!(out.contains("Authorization: $TOKEN"), "{out}");
    }

    #[test]
    fn x_api_key_context() {
        let out = seen(&format!(
            r#"curl -H "X-Api-Key: {SECRET}" https://example.com"#
        ));
        assert!(out.contains("X-Api-Key: ?"), "{out}");
        assert!(!out.contains(SECRET));
    }

    #[test]
    fn access_token_query() {
        let out = seen(&format!(
            r#"curl "https://example.com?access_token={SECRET}""#
        ));
        assert!(out.contains("access_token=?"), "{out}");
        assert!(!out.contains(SECRET));
    }

    #[test]
    fn ghp_prefix() {
        let out = seen("ghp_example000000000000000000000000");
        assert_eq!(out, "?");
        assert!(!out.contains("ghp_example"));
    }

    #[test]
    fn sk_live_prefix() {
        let out = seen("sk-live-example0000000000000000");
        assert_eq!(out, "?");
        assert!(!out.contains("sk-live-example"));
    }

    #[test]
    fn eyj_prefix() {
        let out = seen("eyJexample.not.a.jwt.payload");
        assert!(out.contains('?'), "{out}");
        assert!(!out.contains("eyJexample"));
    }

    #[test]
    fn sha_length_heuristic() {
        let sha = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
        let out = seen(&format!("git checkout {sha}"));
        assert_eq!(out, "git checkout ?");
        assert!(!out.contains(sha));
    }

    #[test]
    fn npm_run_unchanged() {
        assert_eq!(seen("npm run deploy"), "npm run deploy");
    }

    #[test]
    fn access_hydrate_replaces_name() {
        let mut values = HashMap::new();
        values.insert("API_TOKEN".into(), SECRET.into());
        let out = redact_command(&format!("npm run deploy -- --env {SECRET}"), Some(&values));
        assert_eq!(out.command, "npm run deploy -- --env ${API_TOKEN}");
        assert!(!out.command.contains(SECRET));
    }

    #[test]
    fn access_hydrate_plus_leftover_bearer() {
        let mut values = HashMap::new();
        values.insert("API_TOKEN".into(), SECRET.into());
        let raw = format!("deploy {SECRET} -H \"Authorization: Bearer {OTHER}\"");
        let out = redact_command(&raw, Some(&values));
        assert!(out.command.contains("${API_TOKEN}"), "{}", out.command);
        assert!(out.command.contains("Bearer ?"), "{}", out.command);
        assert!(!out.command.contains(SECRET));
        assert!(!out.command.contains(OTHER));
    }

    #[test]
    fn leftover_hydrate_stores_empty() {
        let mut values = HashMap::new();
        values.insert("API_TOKEN".into(), "API".into());
        let out = redact_command("echo API", Some(&values));
        assert_eq!(out.command, "");
    }

    #[test]
    fn cap_1024_sets_truncated() {
        let raw: String = "a".repeat(1025);
        let (out, truncated) = cap_text(raw);
        assert_eq!(out.chars().count(), 1024);
        assert!(truncated);
    }

    #[test]
    fn peel_command_splits_like_docker() {
        assert_eq!(
            peel_command("npm run deploy"),
            ("npm".into(), Some(json!(["run", "deploy"])))
        );
        assert_eq!(
            peel_command(r#"echo hello | grep foo"#),
            ("echo".into(), Some(json!(["hello", "|", "grep", "foo"])))
        );
        assert_eq!(
            peel_command(r#"sh -c "echo hello | grep foo""#),
            ("sh".into(), Some(json!(["-c", "echo hello | grep foo"])))
        );
        assert_eq!(peel_command("ls"), ("ls".into(), None));
    }

    #[test]
    fn auth_schemes_only_after_authorization() {
        let out = seen(r#"curl -H "X-Api-Key: basic hunter2""#);
        assert!(out.contains("X-Api-Key: ?"), "{out}");
        assert!(out.contains("hunter2"), "{out}");
        let auth = seen(r#"curl -H "Authorization: basic hunter2""#);
        assert!(auth.contains("Authorization: basic ?"), "{auth}");
        assert!(!auth.contains("hunter2"), "{auth}");
    }

    #[test]
    fn json_args_are_scrubbed_and_capped() {
        let sha = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
        let out = redact_argv(Some(json!({"token": sha, "ok": "lade"})), None).unwrap();
        assert_eq!(out["token"], "?");
        assert_eq!(out["ok"], "lade");
        let huge = "a".repeat(2000);
        let capped = redact_argv(Some(json!({"z": huge, "a": "keep"})), None).unwrap();
        assert_eq!(capped["z"].as_str().unwrap().chars().count(), 1024);
        assert_eq!(capped["a"], "keep");
    }

    #[test]
    fn argv_hydrate_leftover_drops_column() {
        let mut values = HashMap::new();
        values.insert("API_TOKEN".into(), "API".into());
        assert!(redact_argv(Some(json!(["echo", "API"])), Some(&values)).is_none());
        let kept = redact_argv(Some(json!(["engram", "--stdio"])), None).unwrap();
        assert_eq!(kept, json!(["engram", "--stdio"]));
    }
}
