#![cfg(unix)]

//! A pre-exec command whose fixed tunnel port another program holds must not
//! run: it would reach that program. Each shell runs its real `lade on`
//! hooks around one command, then a plain command that must work again.
//! The shell says Lade cancelled the command, and the user's own
//! command-not-found handler answers again afterward.
//! A debug build writes `cargo run --` in its hooks, put back to `lade` here.

use std::net::TcpListener;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

const ZSH: &str = r#"
command_not_found_handler() { print -u2 "user handler: $1"; return 127; }
eval "$(lade on | sed 's/cargo run -- /lade /g')"
for f in $preexec_functions; do $f "touch $MARKER"; done
touch "$MARKER"
for f in $precmd_functions; do $f; done
lade-missing-command
touch "$AFTER"
"#;

const BASH: &str = r#"
command_not_found_handle() { echo "user handler: $1" >&2; return 127; }
eval "$(lade on | sed 's/cargo run -- /lade /g')"
preexec_lade "touch $MARKER"
touch "$MARKER"
for f in "${precmd_functions[@]}"; do "$f"; done
lade-missing-command
touch "$AFTER"
"#;

const FISH: &str = r#"
function fish_command_not_found; echo "user handler: $argv[1]" >&2; end
lade on | string replace -a 'cargo run -- ' 'lade ' | source
emit fish_preexec "touch $MARKER"
touch $MARKER
emit fish_postexec "touch $MARKER"
lade-missing-command
touch $AFTER
"#;

/// Which failure the command meets.
enum Failure {
    PortHeld,
    /// A cache path too long for a Unix socket, so no hub can start.
    NoHub,
}

/// bash before 4.0 (macOS's /bin/bash) has no `command_not_found_handle`:
/// it names the missing command itself, and the user's handler never runs.
fn has_not_found_handler(shell: &str) -> bool {
    if shell != "bash" {
        return true;
    }
    let version = Command::new("bash")
        .args(["-c", "echo ${BASH_VERSINFO[0]}"])
        .output()
        .unwrap();
    let major: u32 = String::from_utf8_lossy(&version.stdout)
        .trim()
        .parse()
        .unwrap();
    major >= 4
}

fn run(shell: &str, script: &str, failure: Failure) {
    let home = tempdir().unwrap();
    let project = tempdir().unwrap();
    let holder = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = holder.local_addr().unwrap().port();
    std::fs::write(
        project.path().join("lade.yml"),
        format!("\"touch.*\":\n  {port}: ssh://jump.example.com/db.internal/5432\n"),
    )
    .unwrap();
    let config = home.path().join("lade-config.json");
    std::fs::write(&config, r#"{"user":null}"#).unwrap();
    let marker = home.path().join("ran");
    let after = home.path().join("after");
    let tmp = home.path().join("tmp");
    std::fs::create_dir(&tmp).unwrap();
    let lade_dir = Path::new(env!("CARGO_BIN_EXE_lade")).parent().unwrap();
    let path = format!("{}:{}", lade_dir.display(), std::env::var("PATH").unwrap());
    let (cache, expected) = match failure {
        // The hub socket path must stay under the 104-byte limit.
        Failure::PortHeld => (
            home.path().join("c"),
            format!("Local port {port} is already in use"),
        ),
        Failure::NoHub => (
            home.path()
                .join("a-cache-directory-whose-path-is-far-too-long-for-a-unix-socket-to-bind"),
            "Tunnels run in the Lade hub, and it is down".to_string(),
        ),
    };
    let output = Command::new(shell)
        .args(["-c", script])
        .current_dir(project.path())
        .env_clear()
        .env("PATH", path)
        .env("HOME", home.path())
        .env("LADE_CONFIG_PATH", &config)
        .env("LADE_CACHE_DIR", cache)
        .env("LADE_EVENTS_PATH", home.path().join("events.db"))
        .env("LADE_TICKET_DIR", home.path().join("t"))
        .env("MARKER", &marker)
        .env("AFTER", &after)
        .env("TMPDIR", &tmp)
        .output()
        .unwrap();
    let tickets = home.path().join("t");
    assert_command_blocked(shell, &output, &marker, &after, &tmp, &tickets, &expected);
}

fn write_fake_mise(dir: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join("mise");
    std::fs::write(
        &path,
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2024.8.12"
  exit 0
fi
exit 0
"#,
    )
    .unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn run_pin(shell: &str, script: &str) {
    let home = tempdir().unwrap();
    let project = tempdir().unwrap();
    std::fs::write(
        project.path().join("lade.yml"),
        "\"touch.*\":\n  touch: mise://core/not-a-real-tool@0.0.0\n",
    )
    .unwrap();
    let config = home.path().join("lade-config.json");
    std::fs::write(&config, r#"{"user":null}"#).unwrap();
    let marker = home.path().join("ran");
    let after = home.path().join("after");
    let tmp = home.path().join("tmp");
    std::fs::create_dir(&tmp).unwrap();
    let mise_bin = home.path().join("bin");
    write_fake_mise(&mise_bin);
    let lade_dir = Path::new(env!("CARGO_BIN_EXE_lade")).parent().unwrap();
    let path = format!(
        "{}:{}:{}",
        lade_dir.display(),
        mise_bin.display(),
        std::env::var("PATH").unwrap()
    );
    let output = Command::new(shell)
        .args(["-c", script])
        .current_dir(project.path())
        .env_clear()
        .env("PATH", path)
        .env("HOME", home.path())
        .env("LADE_CONFIG_PATH", &config)
        .env("LADE_CACHE_DIR", home.path().join("c"))
        .env("LADE_EVENTS_PATH", home.path().join("events.db"))
        .env("LADE_TICKET_DIR", home.path().join("t"))
        .env("MISE_DATA_DIR", home.path().join("mise-data"))
        .env("MISE_INSTALLS_DIR", home.path().join("mise-installs"))
        .env("MARKER", &marker)
        .env("AFTER", &after)
        .env("TMPDIR", &tmp)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("mise did not install"), "{shell}: {stderr}");
    assert_command_blocked(
        shell,
        &output,
        &marker,
        &after,
        &tmp,
        &home.path().join("t"),
        "Could not run the locked touch",
    );
}

fn assert_command_blocked(
    shell: &str,
    output: &std::process::Output,
    marker: &Path,
    after: &Path,
    tmp: &Path,
    tickets: &Path,
    expected: &str,
) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(expected), "{shell}: {stderr}");
    assert!(
        !marker.exists(),
        "{shell} ran the blocked command: {stderr}"
    );
    assert!(after.exists(), "{shell} kept the command blocked: {stderr}");
    if has_not_found_handler(shell) {
        // Anything else run while blocked, such as the hook's `lade unset`
        // or psub's `rm`, would add a line.
        assert_eq!(
            stderr.matches("Lade cancelled").collect::<Vec<_>>().len(),
            1,
            "{shell}: {stderr}"
        );
        assert!(
            stderr.contains("Lade cancelled touch."),
            "{shell}: {stderr}"
        );
        assert!(
            stderr.contains("user handler: lade-missing-command"),
            "{shell} lost the user's handler: {stderr}"
        );
    } else {
        assert!(
            stderr.contains("touch: command not found"),
            "{shell}: {stderr}"
        );
        assert!(
            !stderr.contains("lade: command not found"),
            "{shell} hook ran lade while blocked: {stderr}"
        );
    }
    assert_eq!(
        std::fs::read_dir(tmp).unwrap().count(),
        0,
        "{shell} left temp files: {stderr}"
    );
    assert_eq!(
        std::fs::read_dir(tickets).map_or(0, |dir| dir.count()),
        0,
        "{shell} left a ticket: {stderr}"
    );
}

#[test]
fn zsh_does_not_run_a_command_whose_port_is_held() {
    run("zsh", ZSH, Failure::PortHeld);
}

#[test]
fn bash_does_not_run_a_command_whose_port_is_held() {
    run("bash", BASH, Failure::PortHeld);
}

#[test]
fn fish_does_not_run_a_command_whose_port_is_held() {
    run("fish", FISH, Failure::PortHeld);
}

#[test]
fn without_a_hub_a_command_with_a_tunnel_does_not_run() {
    run("zsh", ZSH, Failure::NoHub);
}

#[test]
fn zsh_does_not_run_a_command_whose_pin_refuses() {
    run_pin("zsh", ZSH);
}

#[test]
fn bash_does_not_run_a_command_whose_pin_refuses() {
    run_pin("bash", BASH);
}

#[test]
fn fish_does_not_run_a_command_whose_pin_refuses() {
    run_pin("fish", FISH);
}

// The withheld line goes through the hook, then `lade approve` with the code
// the disclaimer printed, through the hook too, which skips `lade` lines.
// `sh` is external, so the cancel's `PATH` stops it. The shell's stderr is
// `$ERR`, where the script reads the code: fish event handlers ignore a
// redirection around `emit`.
const ZSH_APPROVE: &str = r#"
eval "$(lade on | sed 's/cargo run -- /lade /g')"
for f in $preexec_functions; do $f "$LINE"; done
eval "$LINE"
for f in $precmd_functions; do $f; done
[ -e "$MARKER" ] && echo "ran before approve" >&2
code=$(sed -n 's/.*LADE_APPROVE=\([0-9a-f]*\).*/\1/p' "$ERR" | head -n 1)
for f in $preexec_functions; do $f "lade approve $code"; done
lade approve "$code"
for f in $precmd_functions; do $f; done
"#;

const BASH_APPROVE: &str = r#"
eval "$(lade on | sed 's/cargo run -- /lade /g')"
preexec_lade "$LINE"
eval "$LINE"
for f in "${precmd_functions[@]}"; do "$f"; done
[ -e "$MARKER" ] && echo "ran before approve" >&2
code=$(sed -n 's/.*LADE_APPROVE=\([0-9a-f]*\).*/\1/p' "$ERR" | head -n 1)
preexec_lade "lade approve $code"
lade approve "$code"
for f in "${precmd_functions[@]}"; do "$f"; done
"#;

const FISH_APPROVE: &str = r#"
lade on | string replace -a 'cargo run -- ' 'lade ' | source
emit fish_preexec $LINE
eval $LINE
emit fish_postexec $LINE
test -e $MARKER; and echo "ran before approve" >&2
set code (string match -r -g 'LADE_APPROVE=([0-9a-f]+)' < $ERR)[1]
emit fish_preexec "lade approve $code"
lade approve $code
emit fish_postexec "lade approve $code"
"#;

fn run_approve(shell: &str, script: &str) {
    let home = tempdir().unwrap();
    let project = tempdir().unwrap();
    std::fs::write(
        project.path().join("lade.yml"),
        "\"^sh -c\":\n  \".\":\n    disclaimer: \"Danger!\"\n  SECRET: val\n",
    )
    .unwrap();
    let config = home.path().join("lade-config.json");
    std::fs::write(&config, r#"{"user":null}"#).unwrap();
    let marker = home.path().join("ran");
    let err = home.path().join("hook.err");
    let tickets = home.path().join("t");
    let lade_dir = Path::new(env!("CARGO_BIN_EXE_lade")).parent().unwrap();
    let path = format!("{}:{}", lade_dir.display(), std::env::var("PATH").unwrap());
    let output = Command::new(shell)
        .args(["-c", script])
        .current_dir(project.path())
        .env_clear()
        .env("PATH", path)
        .env("HOME", home.path())
        .env("LADE_CONFIG_PATH", &config)
        .env("LADE_CACHE_DIR", home.path().join("c"))
        .env("LADE_EVENTS_PATH", home.path().join("events.db"))
        .env("LADE_TICKET_DIR", &tickets)
        .env("MARKER", &marker)
        .env("ERR", &err)
        .env("LINE", r#"sh -c 'echo "ran $SECRET" >> "$MARKER"'"#)
        .stderr(std::fs::File::create(&err).unwrap())
        .status()
        .unwrap();
    let stderr = std::fs::read_to_string(&err).unwrap();
    assert!(output.success(), "{shell}: {stderr}");
    assert!(stderr.contains("Disclaimer required"), "{shell}: {stderr}");
    assert!(!stderr.contains("ran before approve"), "{shell}: {stderr}");
    if has_not_found_handler(shell) {
        assert!(stderr.contains("Lade cancelled sh."), "{shell}: {stderr}");
    }
    assert_eq!(
        std::fs::read_to_string(&marker).unwrap_or_default(),
        "ran val\n",
        "{shell}: {stderr}"
    );
    assert_eq!(
        std::fs::read_dir(&tickets).map_or(0, |dir| dir.count()),
        0,
        "{shell} left a ticket: {stderr}"
    );
}

#[test]
fn zsh_runs_a_withheld_command_once_lade_approve_gets_its_code() {
    run_approve("zsh", ZSH_APPROVE);
}

#[test]
fn bash_runs_a_withheld_command_once_lade_approve_gets_its_code() {
    run_approve("bash", BASH_APPROVE);
}

#[test]
fn fish_runs_a_withheld_command_once_lade_approve_gets_its_code() {
    run_approve("fish", FISH_APPROVE);
}

// A line that evaluates `lade off`, other than the exact README line the
// hook skips, is a line the hook wraps, and the after-hook it removes would
// have restored the shell.
const ZSH_OFF: &str = r#"
eval "$(lade on | sed 's/cargo run -- /lade /g')"
for f in $preexec_functions; do $f 'eval "$(lade off)";'; done
eval "$(lade off)";
for f in $precmd_functions; do $f; done
echo "SECRET=${SECRET-none}"
"#;

const BASH_OFF: &str = r#"
eval "$(lade on | sed 's/cargo run -- /lade /g')"
preexec_lade 'eval "$(lade off)";'
eval "$(lade off)";
for f in "${precmd_functions[@]}"; do "$f"; done
echo "SECRET=${SECRET-none}"
"#;

const FISH_OFF: &str = r#"
lade on | string replace -a 'cargo run -- ' 'lade ' | source
emit fish_preexec 'eval (lade off)'
eval (lade off)
emit fish_postexec 'eval (lade off)'
if set -q SECRET; echo "SECRET=$SECRET"; else; echo "SECRET=none"; end
"#;

fn run_off(shell: &str, script: &str) {
    let home = tempdir().unwrap();
    let project = tempdir().unwrap();
    std::fs::write(project.path().join("lade.yml"), "\".\":\n  SECRET: val\n").unwrap();
    let config = home.path().join("lade-config.json");
    std::fs::write(&config, r#"{"user":null}"#).unwrap();
    let lade_dir = Path::new(env!("CARGO_BIN_EXE_lade")).parent().unwrap();
    let path = format!("{}:{}", lade_dir.display(), std::env::var("PATH").unwrap());
    let output = Command::new(shell)
        .args(["-c", script])
        .current_dir(project.path())
        .env_clear()
        .env("PATH", path)
        .env("HOME", home.path())
        .env("LADE_CONFIG_PATH", &config)
        .env("LADE_CACHE_DIR", home.path().join("c"))
        .env("LADE_EVENTS_PATH", home.path().join("events.db"))
        .env("LADE_TICKET_DIR", home.path().join("t"))
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{shell}: {stderr}");
    assert!(stdout.contains("SECRET=none"), "{shell}: {stdout} {stderr}");
}

#[test]
fn zsh_lade_off_gives_the_shell_its_environment_back() {
    run_off("zsh", ZSH_OFF);
}

#[test]
fn bash_lade_off_gives_the_shell_its_environment_back() {
    run_off("bash", BASH_OFF);
}

#[test]
fn fish_lade_off_gives_the_shell_its_environment_back() {
    run_off("fish", FISH_OFF);
}

const BROKEN_YAML: &str = "\"touch.*\": [unclosed\n";

/// The shell env of a hooked shell in `project`, whose `lade.yml` is broken.
fn unloaded_shell(shell: &str, script: &str, home: &Path, project: &Path) -> Command {
    std::fs::write(project.join("lade.yml"), BROKEN_YAML).unwrap();
    let config = home.join("lade-config.json");
    std::fs::write(&config, r#"{"user":null}"#).unwrap();
    let tmp = home.join("tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let lade_dir = Path::new(env!("CARGO_BIN_EXE_lade")).parent().unwrap();
    let path = format!("{}:{}", lade_dir.display(), std::env::var("PATH").unwrap());
    let mut cmd = Command::new(shell);
    cmd.args(["-c", script])
        .current_dir(project)
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .env("LADE_CONFIG_PATH", &config)
        .env("LADE_CACHE_DIR", home.join("c"))
        .env("LADE_EVENTS_PATH", home.join("events.db"))
        .env("LADE_TICKET_DIR", home.join("t"))
        .env("MARKER", home.join("ran"))
        .env("AFTER", home.join("after"))
        .env("TMPDIR", &tmp);
    cmd
}

fn run_unloaded(shell: &str, script: &str) {
    let (home, project) = (tempdir().unwrap(), tempdir().unwrap());
    let output = unloaded_shell(shell, script, home.path(), project.path())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("lade.yml"), "{shell}: {stderr}");
    assert!(
        stderr.contains("eval \"$(lade off)\""),
        "{shell} gave no way out: {stderr}"
    );
    assert_command_blocked(
        shell,
        &output,
        &home.path().join("ran"),
        &home.path().join("after"),
        &home.path().join("tmp"),
        &home.path().join("t"),
        "Could not parse a lade.yaml.",
    );
}

#[test]
fn zsh_cancels_a_line_while_lade_yaml_does_not_load() {
    run_unloaded("zsh", ZSH);
}

#[test]
fn bash_cancels_a_line_while_lade_yaml_does_not_load() {
    run_unloaded("bash", BASH);
}

#[test]
fn fish_cancels_a_line_while_lade_yaml_does_not_load() {
    run_unloaded("fish", FISH);
}

// The way out the refusal names: the README line runs unwrapped, so it
// removes the hook while every wrapped line is cancelled.
const ZSH_UNLOADED_OFF: &str = r#"
eval "$(lade on | sed 's/cargo run -- /lade /g')"
for f in $preexec_functions; do $f 'eval "$(lade off)"'; done
eval "$(lade off)"
for f in $precmd_functions; do $f; done
(( $+functions[preexec_lade] )) && echo "hook still on" >&2
for f in $preexec_functions; do $f "touch $MARKER"; done
touch "$MARKER"
"#;

const BASH_UNLOADED_OFF: &str = r#"
eval "$(lade on | sed 's/cargo run -- /lade /g')"
for f in "${preexec_functions[@]}"; do "$f" 'eval "$(lade off)"'; done
eval "$(lade off)"
for f in "${precmd_functions[@]}"; do "$f"; done
declare -F preexec_lade >/dev/null && echo "hook still on" >&2
for f in "${preexec_functions[@]}"; do "$f" "touch $MARKER"; done
touch "$MARKER"
"#;

const FISH_UNLOADED_OFF: &str = r#"
lade on | string replace -a 'cargo run -- ' 'lade ' | source
emit fish_preexec 'eval "$(lade off)"'
eval "$(lade off)"
emit fish_postexec 'eval "$(lade off)"'
functions -q preexec_lade; and echo "hook still on" >&2
emit fish_preexec "touch $MARKER"
touch $MARKER
emit fish_postexec "touch $MARKER"
"#;

fn run_unloaded_off(shell: &str, script: &str) {
    let (home, project) = (tempdir().unwrap(), tempdir().unwrap());
    let output = unloaded_shell(shell, script, home.path(), project.path())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{shell}: {stderr}");
    assert!(!stderr.contains("hook still on"), "{shell}: {stderr}");
    assert!(!stderr.contains("Lade cancelled"), "{shell}: {stderr}");
    assert!(home.path().join("ran").exists(), "{shell}: {stderr}");
}

#[test]
fn zsh_lade_off_runs_while_lade_yaml_does_not_load() {
    run_unloaded_off("zsh", ZSH_UNLOADED_OFF);
}

#[test]
fn bash_lade_off_runs_while_lade_yaml_does_not_load() {
    run_unloaded_off("bash", BASH_UNLOADED_OFF);
}

#[test]
fn fish_lade_off_runs_while_lade_yaml_does_not_load() {
    run_unloaded_off("fish", FISH_UNLOADED_OFF);
}
