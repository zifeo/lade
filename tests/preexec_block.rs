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
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&expected), "{shell}: {stderr}");
    assert!(
        !marker.exists(),
        "{shell} ran the blocked command: {stderr}"
    );
    assert!(after.exists(), "{shell} kept the command blocked: {stderr}");
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
    assert_eq!(
        std::fs::read_dir(&tmp).unwrap().count(),
        0,
        "{shell} left temp files: {stderr}"
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
