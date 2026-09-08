use super::*;
use serde_json::json;
use std::collections::HashMap;

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
