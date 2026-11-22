mod common;

use std::fs;

use tempfile::tempdir;

fn git_repo(dir: &std::path::Path) {
    common::init_git(dir);
}

#[test]
fn setup_from_no_shell_lade_knows_names_it_and_wires_the_repo() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    git_repo(dir.path());
    common::write_yml_raw(dir.path(), "\"^echo\":\n  KEY: raw://hello\n");
    // No shell above Lade: its parent is this test.
    let out = common::lade_std(home.path())
        .current_dir(dir.path())
        .env_remove("CI")
        .args(["setup", "--harness", "cursor"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("unknown"), "{stderr}");
    assert!(stderr.contains("Unsupported shell"), "{stderr}");
    assert!(stderr.contains("lade hook enable --shell"), "{stderr}");
    let hook = fs::read_to_string(dir.path().join(".cursor").join("hooks.json")).unwrap();
    assert!(hook.contains("lade hook --harness cursor"), "{hook}");
    for profile in [".bashrc", ".zshrc", ".config/fish/config.fish"] {
        assert!(!home.path().join(profile).exists(), "{profile}");
    }
}

#[test]
fn setup_with_a_profile_it_cannot_read_writes_no_hook_and_wires_the_repo() {
    // `.zshrc` holds Lade's line next to a byte that is not UTF-8; `.bashrc`,
    // this shell's own profile, is unreadable the same way.
    for unreadable in [".zshrc", ".bashrc"] {
        let home = tempdir().unwrap();
        let dir = tempdir().unwrap();
        git_repo(dir.path());
        common::write_yml_raw(dir.path(), "\"^echo\":\n  KEY: raw://hello\n");
        let profile = home.path().join(unreadable);
        let content = b"eval \"$(lade on)\"  # lade-do-not-edit\n\xff\n".to_vec();
        fs::write(&profile, &content).unwrap();
        let out = common::lade(home.path())
            .current_dir(dir.path())
            .env_remove("CI")
            .args(["setup", "--harness", "cursor"])
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{unreadable}: {stderr}");
        assert!(
            stderr.contains("Lade cannot read"),
            "{unreadable}: {stderr}"
        );
        assert_eq!(fs::read(&profile).unwrap(), content);
        if unreadable == ".zshrc" {
            assert!(!home.path().join(".bashrc").exists(), "{stderr}");
        }
        let hook = fs::read_to_string(dir.path().join(".cursor").join("hooks.json")).unwrap();
        assert!(hook.contains("lade hook --harness cursor"), "{hook}");
    }
}

#[test]
fn setup_writes_project_pretool_hook() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    git_repo(dir.path());
    common::write_yml_raw(dir.path(), "\"^echo\":\n  KEY: raw://hello\n");

    let setup = common::lade(home.path())
        .current_dir(dir.path())
        .args(["setup", "--harness", "cursor"])
        .assert()
        .success()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8_lossy(&setup);
    assert!(stderr.contains("pre-exec"), "{stderr}");
    assert!(stderr.contains("pre-tool"), "{stderr}");
    assert!(
        stderr.contains("lade hook disable --shell"),
        "setup should say how to remove pre-exec: {stderr}"
    );
    assert!(
        stderr.contains("lade teardown"),
        "setup should say how to remove pre-tool: {stderr}"
    );
    assert!(
        !stderr.contains("packages"),
        "no mise pins in this fixture: {stderr}"
    );
    assert!(
        !stderr.contains('╭'),
        "setup success is a report, not a box: {stderr}"
    );

    let hook = dir.path().join(".cursor").join("hooks.json");
    let body = fs::read_to_string(&hook).unwrap();
    assert!(
        body.contains("lade hook --harness cursor"),
        "project hook should call pretool handler: {body}"
    );

    let pretool = common::lade(home.path())
        .current_dir(dir.path())
        .args(["status", "--json"])
        .assert()
        .success();
    let value: serde_json::Value = serde_json::from_slice(&pretool.get_output().stdout).unwrap();
    assert!(
        value["hooks"]["pretool"]["cursor"]["project"]["installed"]
            .as_bool()
            .unwrap()
    );
    assert!(
        value["hooks"]["pretool"]["cursor"]["project"]["current"]
            .as_bool()
            .unwrap()
    );
}

#[test]
fn teardown_removes_project_pretool_hook() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    git_repo(dir.path());
    common::write_yml_raw(dir.path(), "\"^echo\":\n  KEY: raw://hello\n");
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["hook", "enable", "--shell"])
        .assert()
        .success();
    let bashrc = home.path().join(".bashrc");
    let profile = fs::read_to_string(&bashrc).unwrap();
    assert!(profile.contains("lade-do-not-edit"), "{profile}");
    let lock = "[[tools.jq]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n";
    fs::write(dir.path().join("lade.lock"), lock).unwrap();

    common::lade(home.path())
        .current_dir(dir.path())
        .args(["setup", "--harness", "cursor"])
        .assert()
        .success();
    assert!(dir.path().join(".cursor").join("hooks.json").is_file());

    let teardown = common::lade(home.path())
        .current_dir(dir.path())
        .arg("teardown")
        .assert()
        .success()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8_lossy(&teardown);
    assert!(stderr.contains("removed"), "{stderr}");
    assert!(
        stderr.contains("lade setup"),
        "teardown should say how to put pre-tool back: {stderr}"
    );

    let hook = dir.path().join(".cursor").join("hooks.json");
    if hook.is_file() {
        let body = fs::read_to_string(&hook).unwrap();
        assert!(
            !body.contains("lade hook --harness cursor"),
            "teardown should remove the pretool hook: {body}"
        );
    }
    assert_eq!(fs::read_to_string(&bashrc).unwrap(), profile);
    assert_eq!(
        fs::read_to_string(dir.path().join("lade.lock")).unwrap(),
        lock
    );
    assert!(dir.path().join("lade.yml").is_file());
}

#[test]
fn teardown_global_clears_cache_without_lade_yaml() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let cache = home.path().join("lade-cache");
    fs::create_dir_all(cache.join("tickets")).unwrap();
    fs::write(cache.join("tickets/ab12.json"), "{}").unwrap();
    fs::create_dir_all(cache.join("mise-env")).unwrap();
    fs::write(cache.join("mise-env/marker"), "x").unwrap();

    common::lade(home.path())
        .current_dir(dir.path())
        .args(["teardown", "--global"])
        .assert()
        .success();

    assert!(!cache.exists());
}

#[test]
fn add_writes_binding_and_remove_drops_it() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    git_repo(dir.path());
    common::write_yml_raw(dir.path(), "\"^echo\":\n  KEY: raw://hello\n");

    let added = common::lade(home.path())
        .current_dir(dir.path())
        .args([
            "add",
            "secret",
            "--rule",
            "^deploy",
            "--key",
            "TOKEN",
            "--uri",
            "raw://from-add",
        ])
        .assert()
        .success();
    // Setup's report: `lade add` ran it.
    let added = String::from_utf8_lossy(&added.get_output().stderr).into_owned();
    assert!(added.contains("pre-tool"), "{added}");

    let yaml = fs::read_to_string(dir.path().join("lade.yml")).unwrap();
    assert!(yaml.contains("^deploy"));
    assert!(yaml.contains("TOKEN"));
    assert!(yaml.contains("from-add"));

    let removed = common::lade(home.path())
        .current_dir(dir.path())
        .args(["remove", "secret", "--rule", "^deploy", "--key", "TOKEN"])
        .assert()
        .success();
    let removed = String::from_utf8_lossy(&removed.get_output().stderr).into_owned();
    assert!(!removed.contains("pre-tool"), "{removed}");

    let yaml = fs::read_to_string(dir.path().join("lade.yml")).unwrap();
    assert!(!yaml.contains("^deploy"));
    assert!(!yaml.contains("from-add"));
}

fn add_key(home: &std::path::Path, dir: &std::path::Path, key: &str) {
    common::lade(home)
        .current_dir(dir)
        .args([
            "add", "secret", "--rule", "^deploy", "--key", key, "--uri", "raw://x",
        ])
        .assert()
        .success();
}

/// The release lookup is pointed at a closed port, and asks for a version
/// that does not exist, so no binary is ever replaced.
#[test]
fn upgrade_runs_when_repo_needs_newer_lade() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    common::write_yml_raw(dir.path(), ": >=99.0.0\n\"^echo\":\n  KEY: v\n");
    let mut cmd = common::lade(home.path());
    for proxy in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
    ] {
        cmd.env(proxy, "http://127.0.0.1:9");
    }
    let out = cmd
        .current_dir(dir.path())
        .args(["upgrade", "--yes", "--version", "0.0.0"])
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "{stderr}");
    assert!(
        format!("{stdout}{stderr}").contains("Looking for tag: v0.0.0"),
        "{stdout}{stderr}"
    );
    assert!(!stderr.contains(">=99.0.0"), "{stderr}");
    assert!(!stderr.contains("Could not parse a lade.yaml"), "{stderr}");
    // The same walk refuses a verb that reads it.
    let refused = common::lade(home.path())
        .current_dir(dir.path())
        .args(["inject", "echo", "hi"])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&refused.stderr).contains(">=99.0.0"));
}

#[test]
fn add_with_no_yaml_creates_one_in_the_cwd() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    git_repo(dir.path());
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    add_key(home.path(), &sub, "TOKEN");
    let yaml = fs::read_to_string(sub.join("lade.yaml")).unwrap();
    assert!(yaml.contains("TOKEN"), "{yaml}");
    assert!(!dir.path().join("lade.yaml").exists());
}

#[test]
fn add_without_a_terminal_writes_the_nearest_yaml() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    git_repo(dir.path());
    let sub = dir.path().join("sub");
    fs::create_dir(&sub).unwrap();
    common::write_yml_raw(dir.path(), "\"^echo\":\n  KEY: raw://hello\n");
    common::write_yml_raw(&sub, "\"^echo\":\n  KEY: raw://hello\n");
    add_key(home.path(), &sub, "TOKEN");
    assert!(
        fs::read_to_string(sub.join("lade.yml"))
            .unwrap()
            .contains("TOKEN")
    );
    assert!(
        !fs::read_to_string(dir.path().join("lade.yml"))
            .unwrap()
            .contains("TOKEN")
    );
}

#[test]
fn on_off_emit_shell_snippets() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();

    let on = common::lade(home.path())
        .current_dir(dir.path())
        .arg("on")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let on_text = String::from_utf8_lossy(&on);
    assert!(on_text.contains("preexec_functions+=(preexec_lade)"));

    let off = common::lade(home.path())
        .current_dir(dir.path())
        .arg("off")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let off_text = String::from_utf8_lossy(&off);
    assert!(off_text.contains("preexec_functions"));
}

#[test]
fn update_succeeds_with_existing_lock() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    git_repo(dir.path());
    common::write_yml_raw(dir.path(), "\"^echo\":\n  KEY: raw://hello\n");

    common::lade(home.path())
        .current_dir(dir.path())
        .args(["setup", "--harness", "cursor"])
        .assert()
        .success();

    common::lade(home.path())
        .current_dir(dir.path())
        .arg("update")
        .assert()
        .success();
}

#[test]
fn hook_enable_shell_writes_profile() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let bashrc = home.path().join(".bashrc");

    common::lade(home.path())
        .current_dir(dir.path())
        .args(["hook", "enable", "--shell"])
        .assert()
        .success();

    let content = fs::read_to_string(&bashrc).unwrap();
    assert!(
        content.contains("lade-do-not-edit"),
        "profile should contain the pre-exec marker: {content}"
    );
    assert!(
        content.contains("lade on"),
        "profile wrap must use PATH lade, got: {content}"
    );
    assert!(
        !content.contains("/installs/") && !content.contains("/target/"),
        "profile must not pin an install path: {content}"
    );

    let output = common::lade(home.path())
        .current_dir(dir.path())
        .args(["status", "--json"])
        .assert()
        .success();
    let value: serde_json::Value = serde_json::from_slice(&output.get_output().stdout).unwrap();
    assert!(value["hooks"]["preexec"]["installed"].as_bool().unwrap());
}

#[test]
fn help_hides_internal_injection_commands() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    let output = common::lade(home.path())
        .current_dir(dir.path())
        .arg("-h")
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let help = String::from_utf8_lossy(&output);
    let hidden = ["inject", "hook", "set", "unset"];
    for name in hidden {
        assert!(
            !help
                .lines()
                .any(|line| line.trim_start().starts_with(&format!("{name} "))),
            "{name} should stay hidden: {help}"
        );
    }
    assert!(help.contains("setup"), "{help}");
    assert!(help.contains("remove"), "{help}");
}

/// `lade <args>` started by bash with stdin and stderr on a pty, as from a
/// terminal, `n` typed ahead. Returns whether it succeeded and what the
/// pty showed. A question nobody answers would hang it: that fails here.
fn run_at_pty(
    home: &std::path::Path,
    dir: &std::path::Path,
    args: &[&str],
    envs: &[(&str, &str)],
) -> (bool, String) {
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    let pty = nix::pty::openpty(None, None).unwrap();
    let mut cmd = Command::new("bash");
    cmd.args(["-c", "\"$0\" \"$@\" && true"])
        .arg(assert_cmd::cargo::cargo_bin("lade"))
        .args(args)
        .current_dir(dir);
    let mut cmd = common::with_lade_env(cmd, home);
    cmd.envs(envs.iter().copied())
        .stdin(Stdio::from(pty.slave.try_clone().unwrap()))
        .stderr(Stdio::from(pty.slave.try_clone().unwrap()))
        .stdout(Stdio::null());
    let mut master = fs::File::from(pty.master);
    master.write_all(b"n\n").unwrap();
    let mut child = cmd.spawn().unwrap();
    drop(pty.slave);
    let shown = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&shown);
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        while let Ok(read @ 1..) = master.read(&mut buf) {
            sink.lock().unwrap().extend_from_slice(&buf[..read]);
        }
    });
    let deadline = Instant::now() + Duration::from_secs(60);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            child.kill().unwrap();
            let shown = String::from_utf8_lossy(&shown.lock().unwrap()).into_owned();
            panic!("`lade {}` did not finish: {shown}", args.join(" "));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    std::thread::sleep(Duration::from_millis(100));
    let shown = String::from_utf8_lossy(&shown.lock().unwrap()).into_owned();
    (status.success(), shown)
}

#[test]
fn setup_never_asks_a_harness() {
    let ask = |envs: &[(&str, &str)]| {
        let home = tempdir().unwrap();
        let dir = tempdir().unwrap();
        git_repo(dir.path());
        common::write_yml_raw(dir.path(), "\"^echo\":\n  KEY: raw://hello\n");
        fs::create_dir(home.path().join(".cursor")).unwrap();
        run_at_pty(home.path(), dir.path(), &["setup"], envs)
    };
    let (ok, shown) = ask(&[]);
    assert!(ok, "{shown}");
    assert!(
        shown.contains("Wrap harnesses for Cursor? [Y/n]"),
        "a human is asked: {shown}"
    );
    let (ok, shown) = ask(&[("CLAUDECODE", "1")]);
    assert!(ok, "{shown}");
    assert!(!shown.contains("[Y/n]"), "{shown}");
    assert!(!shown.contains("Which ("), "{shown}");
}

#[test]
fn starting_a_harness_refreshes_its_hook_without_a_hub() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    git_repo(dir.path());
    common::write_yml_raw(dir.path(), "\"^tofu\":\n  KEY: raw://hello\n");
    let hook = dir.path().join(".cursor").join("hooks.json");
    fs::create_dir_all(hook.parent().unwrap()).unwrap();
    let stale = r#"{"version":1,"hooks":{"preToolUse":[{"command":"lade hook"}]}}"#;
    fs::write(&hook, stale).unwrap();

    let (ok, shown) = run_at_pty(home.path(), dir.path(), &["set", "cursor-agent"], &[]);
    assert!(ok, "{shown}");
    let rewritten = fs::read_to_string(&hook).unwrap();
    assert!(
        rewritten.contains("lade hook --harness cursor"),
        "{rewritten}"
    );
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["ttl"])
        .assert()
        .success()
        .stdout(predicates::str::contains("hub: down"));
}

#[test]
fn setup_outside_git_writes_no_lock() {
    let home = tempdir().unwrap();
    let dir = home.path().join("scratch");
    fs::create_dir_all(&dir).unwrap();
    let toml = "[tools]\n";
    fs::write(dir.join("mise.toml"), toml).unwrap();
    common::write_yml_raw(&dir, "\"^jq\":\n  jq: mise://aqua/jqlang/jq@1.7.1\n");

    let out = common::lade(home.path())
        .current_dir(&dir)
        .env("LADE_MISE_FETCH", "0")
        .arg("setup")
        .assert()
        .success()
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8_lossy(&out);
    assert!(stderr.contains("Packages, locks"), "{stderr}");
    assert!(!dir.join("mise.lock").exists());
    assert_eq!(fs::read_to_string(dir.join("mise.toml")).unwrap(), toml);
}

#[cfg(unix)]
#[test]
fn setup_in_home_extends_home_mise_plane() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempdir().unwrap();
    let tools = tempdir().unwrap();
    let installs = tools.path().join("installs");
    let record = tools.path().join("mise-args");
    let mise = tools.path().join("mise");
    fs::write(
        &mise,
        r#"#!/bin/sh
cd_dir="."
if [ "$1" = "--cd" ]; then cd_dir="$2"; shift 2; fi
case "$1" in
  --version) echo "mise 2024.8.12"; exit 0 ;;
  latest|ls-remote) echo "1.7.1"; exit 0 ;;
esac
printf '%s\n' "$*" >> "$MISE_ARGS_RECORD"
case "$*" in *--json-extended*) echo '{}'; exit 0 ;; esac
if [ "$1" = "lock" ]; then
  printf '[[tools.jq]]\nversion = "1.7.1"\nbackend = "aqua:jqlang/jq"\n' > "$cd_dir/mise.lock"
  exit 0
fi
mkdir -p "$MISE_INSTALLS_DIR/jq/1.7.1"
printf '#!/bin/sh\necho PINNED\n' > "$MISE_INSTALLS_DIR/jq/1.7.1/jq"
chmod 755 "$MISE_INSTALLS_DIR/jq/1.7.1/jq"
"#,
    )
    .unwrap();
    fs::set_permissions(&mise, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(home.path().join("mise.toml"), "[tools]\n").unwrap();
    common::write_yml_raw(home.path(), "\"^jq\":\n  jq: mise://aqua/jqlang/jq@1.7.1\n");

    let out = common::lade(home.path())
        .current_dir(home.path())
        .env("LADE_MISE", &mise)
        .env("LADE_MISE_FETCH", "0")
        .env("MISE_ARGS_RECORD", &record)
        .env("MISE_INSTALLS_DIR", &installs)
        .env("MISE_DATA_DIR", tools.path().join("data"))
        .arg("setup")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(
        stderr.contains("mise files in your home folder"),
        "{stderr}"
    );
    let args = fs::read_to_string(&record).unwrap_or_default();
    assert!(args.lines().any(|line| line.starts_with("lock")), "{args}");
}

#[test]
fn teardown_fails_on_a_pin_it_cannot_parse() {
    let home = tempdir().unwrap();
    let dir = tempdir().unwrap();
    git_repo(dir.path());
    common::write_yml_raw(dir.path(), ".:\n  x: \"mise://aqua/owner/x?teardown=rm\"\n");

    let out = common::lade(home.path())
        .current_dir(dir.path())
        .arg("teardown")
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{stderr}");
    assert!(stderr.contains("missing @version"), "{stderr}");
}
