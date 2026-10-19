use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const PLAINTEXT: &str = "pq secret from acme";

struct Keys {
    secret: String,
    recipient: String,
}

fn keygen(args: &[&str]) -> Keys {
    let out = Command::new("age-keygen")
        .args(args)
        .output()
        .expect("age-keygen from age 1.3.2 on PATH, pinned in lade.yml");
    assert!(out.status.success(), "{}", stderr(&out));
    let text = String::from_utf8(out.stdout).unwrap();
    let secret = text
        .lines()
        .find(|l| l.starts_with("AGE-SECRET-KEY-"))
        .expect("age-keygen prints a secret")
        .to_string();
    let recipient = text
        .lines()
        .find_map(|l| l.strip_prefix("# public key: "))
        .expect("age-keygen prints the public key")
        .to_string();
    Keys { secret, recipient }
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

// Not env!("CARGO_BIN_EXE_age-plugin-lade"): .cargo/sccache-wrapper runs
// rustc through /bin/sh, and dash (Ubuntu) drops env names with a hyphen.
fn plugin_bin() -> PathBuf {
    assert_cmd::cargo::cargo_bin("age-plugin-lade")
}

fn plugin_dir() -> PathBuf {
    plugin_bin().parent().unwrap().to_path_buf()
}

fn workspace_lade() -> PathBuf {
    let lade = plugin_dir().join("lade");
    assert!(
        lade.is_file(),
        "lade binary not found at {}. Run cargo test --workspace --locked.",
        lade.display()
    );
    lade
}

/// Go `age` resolves `age-plugin-lade` on PATH, so the built plugin goes first.
fn age(dir: &Path, args: &[&str], stdin: &[u8]) -> Output {
    let path = std::env::join_paths(
        std::iter::once(plugin_dir())
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
    )
    .unwrap();
    let mut child = Command::new("age")
        .args(args)
        .env("PATH", path)
        .env("LADE_BIN", workspace_lade())
        .env("LADE_EVENTS_PATH", dir.join("events.db"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("age from age 1.3.2 on PATH, pinned in lade.yml");
    child.stdin.take().unwrap().write_all(stdin).unwrap();
    child.wait_with_output().unwrap()
}

/// `(AGE-PLUGIN-LADE-1…, age1lade1…)` for a file URI that hydrates to `secret`.
fn lade_keys(dir: &Path, secret: &str) -> (PathBuf, String) {
    let json = dir.join("key.json");
    std::fs::write(&json, format!(r#"{{"key":"{secret}"}}"#)).unwrap();
    let uri = format!("file://{}?query=.key", json.display());
    let out = Command::new(plugin_bin()).arg(&uri).output().unwrap();
    assert!(out.status.success(), "{}", stderr(&out));
    let text = String::from_utf8(out.stdout).unwrap();
    let recipient = text
        .lines()
        .find_map(|l| l.strip_prefix("# recipient: "))
        .expect("age-plugin-lade prints its recipient")
        .to_string();
    let identity = dir.join("lade-identity.txt");
    std::fs::write(&identity, &text).unwrap();
    (identity, recipient)
}

fn assert_ok(out: &Output) -> String {
    assert!(out.status.success(), "{}", stderr(out));
    String::from_utf8(out.stdout.clone()).unwrap()
}

#[test]
fn go_age_decrypts_pq_file_through_lade() {
    let dir = tempfile::tempdir().unwrap();
    let keys = keygen(&["-pq"]);
    let (lade_identity, _) = lade_keys(dir.path(), &keys.secret);
    let encrypted = dir.path().join("msg.age");

    assert_ok(&age(
        dir.path(),
        &["-r", &keys.recipient, "-o", encrypted.to_str().unwrap()],
        PLAINTEXT.as_bytes(),
    ));
    let plaintext = assert_ok(&age(
        dir.path(),
        &[
            "-d",
            "-i",
            lade_identity.to_str().unwrap(),
            encrypted.to_str().unwrap(),
        ],
        b"",
    ));
    assert_eq!(plaintext, PLAINTEXT);
}

#[test]
fn go_age_encrypts_pq_file_through_lade() {
    let dir = tempfile::tempdir().unwrap();
    let keys = keygen(&["-pq"]);
    let (_, lade_recipient) = lade_keys(dir.path(), &keys.secret);
    let encrypted = dir.path().join("msg.age");
    let native_identity = dir.path().join("pq-identity.txt");
    std::fs::write(&native_identity, format!("{}\n", keys.secret)).unwrap();

    assert_ok(&age(
        dir.path(),
        &["-r", &lade_recipient, "-o", encrypted.to_str().unwrap()],
        PLAINTEXT.as_bytes(),
    ));
    let header = std::fs::read(&encrypted).unwrap();
    assert!(
        header.windows(17).any(|w| w == b"-> mlkem768x25519"),
        "expected a mlkem768x25519 stanza"
    );
    let plaintext = assert_ok(&age(
        dir.path(),
        &[
            "-d",
            "-i",
            native_identity.to_str().unwrap(),
            encrypted.to_str().unwrap(),
        ],
        b"",
    ));
    assert_eq!(plaintext, PLAINTEXT);
}

#[test]
fn go_age_mixes_lade_pq_only_with_pq_recipients() {
    let dir = tempfile::tempdir().unwrap();
    let keys = keygen(&["-pq"]);
    let native_pq = keygen(&["-pq"]);
    let classic = keygen(&[]);
    let (_, lade_recipient) = lade_keys(dir.path(), &keys.secret);
    let encrypted = dir.path().join("msg.age");

    assert_ok(&age(
        dir.path(),
        &[
            "-r",
            &lade_recipient,
            "-r",
            &native_pq.recipient,
            "-o",
            encrypted.to_str().unwrap(),
        ],
        PLAINTEXT.as_bytes(),
    ));
    let mixed = age(
        dir.path(),
        &["-r", &lade_recipient, "-r", &classic.recipient],
        PLAINTEXT.as_bytes(),
    );
    assert!(!mixed.status.success(), "a classic recipient must not join");
    assert!(
        stderr(&mixed).contains("incompatible"),
        "{}",
        stderr(&mixed)
    );
}
