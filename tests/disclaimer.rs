mod common;
use predicates::prelude::PredicateBooleanExt;
use std::fs;
use std::path::Path;
use std::process::Output;
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

    let tickets = tempdir().unwrap();
    // 1. set blocked: a single disclaimer box (not a second loader-shaped error),
    //    carrying the per-command approval code.
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .args(["set", "deploy"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("unset -v LADE_RESTORE"));
    assert!(stdout.contains("export LADE_T='"));
    assert!(!stdout.contains("export LADE_PENDING='"));
    assert!(stderr.contains("Disclaimer required. Access is withheld"));
    assert!(stderr.contains("> Danger!"));
    assert!(!stderr.contains("could not get secrets"));
    let code = extract_code(&stderr);

    // 2. set approved with the exact code
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .env("LADE_APPROVE", &code)
        .args(["set", "deploy"])
        .assert()
        .success()
        .stdout(predicates::str::contains("export SECRET='val'"));

    // 3. a wrong code (including the old `1` reflex) stays blocked
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
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

#[test]
fn test_disclaimer_approve_reads_ticket() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let tickets = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"^echo\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: val\n",
    )
    .unwrap();

    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .args(["set", "echo hi"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let id = common::extract_lade_t(&stdout).expect("LADE_T after withhold");
    let code = extract_code(&stderr);

    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .env("LADE_T", &id)
        .args(["approve", &code])
        .assert()
        .success()
        .stdout(predicates::str::contains("hi"));
}

#[test]
fn an_approved_command_gets_no_approval_code_lade_set_itself() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let tickets = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"^echo\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: val\n",
    )
    .unwrap();
    let line = "echo \"approve=${LADE_APPROVE:-none}\"";
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .args(["set", line])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let id = common::extract_lade_t(&String::from_utf8_lossy(&out.stdout)).expect("LADE_T");
    let code = extract_code(&String::from_utf8_lossy(&out.stderr));

    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .env("LADE_T", &id)
        .args(["approve", &code])
        .assert()
        .success()
        .stdout(predicates::str::contains("approve=none"));
}

#[test]
fn a_withheld_command_calls_no_provider() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let fetched = dir.path().join("fetched");
    fs::write(
        dir.path().join("lade.yml"),
        format!(
            "\"^echo\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: \"sh://touch {}; printf x\"\n",
            fetched.display()
        ),
    )
    .unwrap();
    for args in [
        &["inject", "echo hi"][..],
        &["mcp", "--", "echo", "hi"][..],
        &["set", "echo hi"][..],
    ] {
        let out = common::lade(home.path())
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(3), "{args:?}");
        assert!(
            !fetched.exists(),
            "{args:?} ran the provider before consent"
        );
    }
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "echo hi"])
        .output()
        .unwrap();
    let code = extract_code(&String::from_utf8_lossy(&out.stderr));
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_APPROVE", &code)
        .args(["inject", "echo hi"])
        .assert()
        .success();
    assert!(fetched.exists(), "the approved command ran no provider");
}

/// The code `lade` shows for `command` in a 5-minute `window`.
fn code_for_window(command: &str, window: u64) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(command.as_bytes());
    hasher.update(b"\n");
    hasher.update(window.to_le_bytes());
    let digest = hasher.finalize();
    format!("{:02x}{:02x}{:02x}", digest[0], digest[1], digest[2])[..5].to_string()
}

#[test]
fn an_expired_code_approves_nothing() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let tickets = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"^echo\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: val\n",
    )
    .unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .args(["set", "echo hi"])
        .output()
        .unwrap();
    let id = common::extract_lade_t(&String::from_utf8_lossy(&out.stdout)).expect("LADE_T");
    let window = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        / 300;
    let expired = code_for_window("echo hi", window - 2);
    // Five hex digits may collide with a live window's code.
    if [window, window - 1, window + 1]
        .iter()
        .any(|w| code_for_window("echo hi", *w) == expired)
    {
        return;
    }
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .env("LADE_T", &id)
        .args(["approve", &expired])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("Wrong or expired approval code"));
    assert!(!String::from_utf8_lossy(&out.stdout).contains("hi"));
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

#[test]
fn an_agent_gets_no_approval_code() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"deploy\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: val\n",
    )
    .unwrap();
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .args(["--pretool", "--", "deploy"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("Only a human can approve it"), "{stderr}");
    assert!(!stderr.contains("LADE_APPROVE="), "{stderr}");
    assert!(!stderr.contains("lade approve"), "{stderr}");
    let has_code = stderr
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|word| word.len() == 5 && word.bytes().all(|b| b.is_ascii_hexdigit()));
    assert!(!has_code, "{stderr}");
}

/// `lade set` withholds `cd sub && echo hi` typed in `dir`, and returns the
/// ticket id and approval code.
fn withhold_in_sub(home: &Path, dir: &Path, tickets: &Path) -> (String, String) {
    let sub = dir.join("sub");
    fs::create_dir_all(&sub).unwrap();
    fs::write(
        sub.join("lade.yml"),
        "\"^echo\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: val\n",
    )
    .unwrap();
    let out = common::lade(home)
        .current_dir(dir)
        .env("LADE_TICKET_DIR", tickets)
        .args(["set", "--", "cd sub && echo hi"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    let id = common::extract_lade_t(&String::from_utf8_lossy(&out.stdout)).expect("LADE_T");
    (id, extract_code(&String::from_utf8_lossy(&out.stderr)))
}

fn approve(home: &Path, cwd: &Path, tickets: &Path, id: &str, code: &str) -> Output {
    common::lade(home)
        .current_dir(cwd)
        .env("LADE_TICKET_DIR", tickets)
        .env("LADE_T", id)
        .args(["approve", code])
        .output()
        .unwrap()
}

fn assert_refused(out: &Output, reason: &str) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains(reason), "{stderr}");
    assert!(!String::from_utf8_lossy(&out.stdout).contains("hi"));
}

#[test]
fn approve_resolves_the_withheld_line_from_its_own_directory() {
    let dir = tempdir().unwrap();
    let elsewhere = tempdir().unwrap();
    let home = tempdir().unwrap();
    let tickets = tempdir().unwrap();
    let (id, code) = withhold_in_sub(home.path(), dir.path(), tickets.path());
    let out = approve(home.path(), elsewhere.path(), tickets.path(), &id, &code);
    assert_refused(&out, "for a different directory");
    let wrong = if code == "00000" { "11111" } else { "00000" };
    let out = approve(home.path(), dir.path(), tickets.path(), &id, wrong);
    assert_refused(&out, "Wrong or expired approval code");
    let out = approve(home.path(), dir.path(), tickets.path(), &id, &code);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "hi\n");
    let out = approve(home.path(), dir.path(), tickets.path(), &id, &code);
    assert_refused(&out, "Nothing to approve");
}

#[test]
fn a_command_set_in_full_cannot_be_approved() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let tickets = tempdir().unwrap();
    let (_, code) = withhold_in_sub(home.path(), dir.path(), tickets.path());
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .env("LADE_APPROVE", &code)
        .args(["set", "--", "cd sub && echo hi"])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let id = common::extract_lade_t(&String::from_utf8_lossy(&out.stdout)).expect("LADE_T");
    let out = approve(home.path(), dir.path(), tickets.path(), &id, &code);
    assert_refused(&out, "Nothing to approve");
}

#[test]
fn an_approved_command_no_longer_holds_its_ticket() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let tickets = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "\"^test\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: val\n",
    )
    .unwrap();
    // The id is known only once `set` wrote the ticket: the line globs for it.
    let line = format!("test ! -e {}/*.json && echo gone", tickets.path().display());
    let out = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", tickets.path())
        .args(["set", "--", &line])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(3));
    let id = common::extract_lade_t(&String::from_utf8_lossy(&out.stdout)).expect("LADE_T");
    assert!(tickets.path().join(format!("{id}.json")).exists());
    let code = extract_code(&String::from_utf8_lossy(&out.stderr));
    let out = approve(home.path(), dir.path(), tickets.path(), &id, &code);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "gone\n");
}
