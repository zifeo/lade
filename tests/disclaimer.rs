mod common;
use predicates::prelude::PredicateBooleanExt;
use std::fs;
use tempfile::tempdir;

/// Pull the per-command approval code out of the disclaimer message printed to
/// stderr (`...LADE_APPROVE=ab12c...`).
fn extract_code(stderr: &str) -> String {
    let marker = "LADE_APPROVE=";
    let start = stderr.find(marker).expect("approval code in message") + marker.len();
    stderr[start..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect()
}

#[test]
fn test_disclaimer_hook_flow() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"deploy\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: val\n",
    )
    .unwrap();

    // 1. set blocked: a single disclaimer box (not a second loader-shaped error),
    //    carrying the per-command approval code.
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "deploy"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("unset -v LADE_PENDING"));
    assert!(stdout.contains("export LADE_PENDING='v1:"));
    assert!(stderr.contains("Disclaimer required to uncover the secrets"));
    assert!(stderr.contains("> Danger!"));
    assert!(!stderr.contains("could not get secrets"));
    let code = extract_code(&stderr);

    // 2. set approved with the exact code
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_APPROVE", &code)
        .args(["set", "deploy"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export SECRET='val'"));

    // 3. a wrong code (including the old `1` reflex) stays blocked
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_APPROVE", "1")
        .args(["set", "deploy"])
        .assert()
        .failure();

    // 4. approve without pending: a clean educational box, not a raw error chain
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["approve", "zzzzz"])
        .assert()
        .failure()
        .stderr(predicates::str::contains("no disclaimer is pending"))
        .stderr(predicates::str::contains("Caused by").not());
}

// The agent hook rewrites a matching command to `lade inject '<cmd>'`. With an
// unapproved disclaimer, `lade inject` is the single gate: it prints the
// disclaimer to stderr and fails closed (exit 3) instead of running the command.
#[test]
fn test_disclaimer_inject_fail_closed() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"^echo\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: val\n",
    )
    .unwrap();

    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "echo hi"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Danger!"));
    let code = extract_code(&stderr);

    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_APPROVE", &code)
        .args(["inject", "echo hi"])
        .assert()
        .success()
        .stdout(predicates::str::contains("hi"));
}
