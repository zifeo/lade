mod common;
use common::{OTHER, SECRET, inject, log_rows, stored_command, write_yml};
use tempfile::tempdir;

#[test]
fn env_prefix_is_stripped() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(
        home.path(),
        dir.path(),
        &[&format!("TOKEN={SECRET} curl https://example.com")],
    );
    let cmd = stored_command(home.path(), dir.path());
    assert!(cmd.contains("curl https://example.com"), "{cmd}");
    assert!(!cmd.contains(SECRET), "{cmd}");
}

#[test]
fn inject_seen_does_not_store_secret() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "\"^nomatch$\":\n  X: raw://x\n");
    inject(
        home.path(),
        dir.path(),
        &[
            "curl",
            "-H",
            &format!("Authorization: Bearer {SECRET}"),
            "https://example.com",
        ],
    );
    let rows = log_rows(home.path(), dir.path());
    let cmd = rows[0]["command"].as_str().unwrap();
    assert!(cmd.contains("Authorization: Bearer ?"), "{cmd}");
    assert!(!cmd.contains(SECRET), "{cmd}");
    assert_eq!(rows[0]["kind"], "seen");
}

#[test]
fn unexpanded_token_stays() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(
        home.path(),
        dir.path(),
        &["curl", "-H", "Authorization: $TOKEN", "https://example.com"],
    );
    let cmd = stored_command(home.path(), dir.path());
    assert!(cmd.contains("Authorization: $TOKEN"), "{cmd}");
}

#[test]
fn x_api_key_is_redacted() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(
        home.path(),
        dir.path(),
        &[
            "curl",
            "-H",
            &format!("X-Api-Key: {SECRET}"),
            "https://example.com",
        ],
    );
    let cmd = stored_command(home.path(), dir.path());
    assert!(cmd.contains("X-Api-Key: ?"), "{cmd}");
    assert!(!cmd.contains(SECRET), "{cmd}");
}

#[test]
fn access_token_query_is_redacted() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(
        home.path(),
        dir.path(),
        &[&format!("curl https://example.com?access_token={SECRET}")],
    );
    let cmd = stored_command(home.path(), dir.path());
    assert!(cmd.contains("access_token=?"), "{cmd}");
    assert!(!cmd.contains(SECRET), "{cmd}");
}

#[test]
fn ghp_prefix_is_redacted() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(
        home.path(),
        dir.path(),
        &["ghp_example000000000000000000000000"],
    );
    let cmd = stored_command(home.path(), dir.path());
    assert_eq!(cmd, "?");
    assert!(!cmd.contains("ghp_example"));
}

#[test]
fn sk_live_prefix_is_redacted() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(
        home.path(),
        dir.path(),
        &["sk-live-example0000000000000000"],
    );
    let cmd = stored_command(home.path(), dir.path());
    assert_eq!(cmd, "?");
    assert!(!cmd.contains("sk-live-example"));
}

#[test]
fn eyj_prefix_is_redacted() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["eyJexample.not.a.jwt.payload"]);
    let cmd = stored_command(home.path(), dir.path());
    assert!(cmd.contains('?'), "{cmd}");
    assert!(!cmd.contains("eyJexample"));
}

#[test]
fn sha_length_heuristic() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    let sha = "a1b2c3d4e5f60718293a4b5c6d7e8f9012345678";
    inject(home.path(), dir.path(), &["git", "checkout", sha]);
    let cmd = stored_command(home.path(), dir.path());
    assert_eq!(cmd, "git checkout ?");
    assert!(!cmd.contains(sha));
}

#[test]
fn npm_run_unchanged() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "{}\n");
    inject(home.path(), dir.path(), &["npm", "run", "deploy"]);
    let cmd = stored_command(home.path(), dir.path());
    assert_eq!(cmd, "npm run deploy");
}

#[test]
fn access_hydrate_replaces_name() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(
        dir.path(),
        "\"^echo \":\n  API_TOKEN: tok_example_0000000001\n",
    );
    inject(home.path(), dir.path(), &["echo", SECRET]);
    let rows = log_rows(home.path(), dir.path());
    let cmd = rows[0]["command"].as_str().unwrap();
    assert!(cmd.contains("${API_TOKEN}"), "{cmd}");
    assert!(!cmd.contains(SECRET), "{cmd}");
    assert_eq!(rows[0]["kind"], "access");
}

#[test]
fn access_hydrate_plus_leftover_bearer() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(
        dir.path(),
        "\"^deploy\":\n  API_TOKEN: tok_example_0000000001\n",
    );
    inject(
        home.path(),
        dir.path(),
        &[
            "deploy",
            SECRET,
            "-H",
            &format!("Authorization: Bearer {OTHER}"),
        ],
    );
    let cmd = stored_command(home.path(), dir.path());
    assert!(cmd.contains("${API_TOKEN}"), "{cmd}");
    assert!(cmd.contains("Bearer ?"), "{cmd}");
    assert!(!cmd.contains(SECRET), "{cmd}");
    assert!(!cmd.contains(OTHER), "{cmd}");
}

#[test]
fn leftover_hydrate_stores_empty() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    write_yml(dir.path(), "\"^echo \":\n  API_TOKEN: API\n");
    inject(home.path(), dir.path(), &["echo", "API"]);
    let cmd = stored_command(home.path(), dir.path());
    assert_eq!(cmd, "");
}
