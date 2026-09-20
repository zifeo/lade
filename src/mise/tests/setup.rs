use super::*;

#[cfg(unix)]
#[test]
fn setup_pins_installs_and_writes_lock() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    std::fs::create_dir_all(installs.join("jq/1.8.0")).unwrap();
    write_exec(&installs.join("jq/1.8.0/jq"), "echo OTHER");
    write_exec(&stub.join("mise"), isolation_record_stub());
    git_init(dir.path());
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();
            let result = block_on(setup_pins(PinMode::Locked));
            std::env::set_current_dir(prev).unwrap();
            result.unwrap();
            let lock = std::fs::read_to_string(dir.path().join("lade.lock")).unwrap();
            assert!(lock.contains("[[tools.jq]]"), "{lock}");
            assert!(lock.contains("1.7.1"), "{lock}");
            assert!(!lock.contains("1.8.0"), "{lock}");
            assert!(lock.contains("checksum = \"sha256:"), "{lock}");
            let args = std::fs::read_to_string(installs.join("mise-args")).unwrap();
            assert!(args.contains("install"), "{args}");
            assert!(!args.contains("--locked"), "{args}");
            assert!(installs.join("jq/1.7.1/jq").is_file());
        },
    );
}

#[cfg(unix)]
#[test]
fn setup_pins_implies_op() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(
        &stub.join("mise"),
        r#"
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2024.8.12"
  exit 0
fi
printf '%s\n' "$*" >> "$MISE_INSTALLS_DIR/mise-args"
mkdir -p "$MISE_INSTALLS_DIR/op/2.31.1"
printf '#!/bin/sh\necho OP\n' > "$MISE_INSTALLS_DIR/op/2.31.1/op"
chmod 755 "$MISE_INSTALLS_DIR/op/2.31.1/op"
exit 0
"#,
    );
    git_init(dir.path());
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^terraform:\n  TF_VAR_FOO: op://v/i/f\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();
            let result = block_on(setup_pins(PinMode::Locked));
            std::env::set_current_dir(prev).unwrap();
            result.unwrap();
            let lock = std::fs::read_to_string(dir.path().join("lade.lock")).unwrap();
            assert!(lock.contains("[[tools.op]]"), "{lock}");
            assert!(lock.contains("2.31.1"), "{lock}");
            assert!(lock.contains("checksum = \"sha256:"), "{lock}");
        },
    );
}

#[cfg(unix)]
#[test]
fn setup_pins_rewrites_latest_to_an_exact_version() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), isolation_record_stub());
    git_init(dir.path());
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@latest\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();
            let result = block_on(setup_pins(PinMode::Locked));
            std::env::set_current_dir(prev).unwrap();
            result.unwrap();
            let yaml = std::fs::read_to_string(dir.path().join("lade.yaml")).unwrap();
            assert!(yaml.contains("@1.7.1"), "{yaml}");
            assert!(!yaml.contains("@latest"), "{yaml}");
            let lock = std::fs::read_to_string(dir.path().join("lade.lock")).unwrap();
            assert!(lock.contains("1.7.1"), "{lock}");
            assert!(!lock.contains("latest"), "{lock}");
        },
    );
}

#[cfg(unix)]
#[test]
fn setup_writes_one_lock_at_git_root() {
    let root = tempdir().unwrap();
    let home = tempdir().unwrap();
    let child = root.path().join("app");
    let installs = root.path().join("installs");
    let stub = root.path().join("stub");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    git_init(root.path());
    write_exec(
        &stub.join("mise"),
        r#"
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2024.8.12"
  exit 0
fi
printf '%s\n' "$*" >> "$MISE_INSTALLS_DIR/mise-args"
if printf '%s' "$*" | grep -q jq; then
  mkdir -p "$MISE_INSTALLS_DIR/jq/1.7.1"
  printf '#!/bin/sh\necho PINNED\n' > "$MISE_INSTALLS_DIR/jq/1.7.1/jq"
  chmod 755 "$MISE_INSTALLS_DIR/jq/1.7.1/jq"
fi
if printf '%s' "$*" | grep -q op; then
  mkdir -p "$MISE_INSTALLS_DIR/op/2.31.1"
  printf '#!/bin/sh\necho OP\n' > "$MISE_INSTALLS_DIR/op/2.31.1/op"
  chmod 755 "$MISE_INSTALLS_DIR/op/2.31.1/op"
fi
exit 0
"#,
    );
    std::fs::write(
        root.path().join("lade.yaml"),
        "^terraform:\n  TF_VAR_FOO: op://v/i/f\n",
    )
    .unwrap();
    std::fs::write(
        child.join("lade.yaml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(&child).unwrap();
            let result = block_on(setup_pins(PinMode::Locked));
            std::env::set_current_dir(prev).unwrap();
            result.unwrap();
            let lock = std::fs::read_to_string(root.path().join("lade.lock")).unwrap();
            assert!(lock.contains("[[tools.op]]"), "{lock}");
            assert!(lock.contains("[[tools.jq]]"), "{lock}");
            assert!(!child.join("lade.lock").exists());
        },
    );
}

#[cfg(unix)]
#[test]
fn setup_locked_installs_lock_not_a_newer_store() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    std::fs::create_dir_all(installs.join("jq/1.7.1")).unwrap();
    std::fs::create_dir_all(installs.join("jq/1.8.0")).unwrap();
    write_exec(&installs.join("jq/1.7.1/jq"), "echo LOCKED");
    write_exec(&installs.join("jq/1.8.0/jq"), "echo NEWER");
    write_exec(&stub.join("mise"), isolation_record_stub());
    git_init(dir.path());
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@>=1.7.0\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.lock"),
        "[[tools.jq]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();
            let result = block_on(setup_pins(PinMode::Locked));
            std::env::set_current_dir(prev).unwrap();
            result.unwrap();
            let lock = std::fs::read_to_string(dir.path().join("lade.lock")).unwrap();
            assert!(lock.contains("1.7.1"), "{lock}");
            assert!(!lock.contains("1.8.0"), "{lock}");
            let args = std::fs::read_to_string(installs.join("mise-args")).unwrap();
            assert!(args.contains("--locked"), "{args}");
        },
    );
}

#[cfg(unix)]
#[test]
fn update_bumps_implied_to_latest() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    std::fs::create_dir_all(installs.join("op/2.31.1")).unwrap();
    write_exec(&installs.join("op/2.31.1/op"), "echo OLD");
    write_exec(
        &stub.join("mise"),
        r#"
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2024.8.12"
  exit 0
fi
if [ "$1" = "latest" ]; then
  printf '%s\n' "2.40.0"
  exit 0
fi
printf '%s\n' "$*" >> "$MISE_INSTALLS_DIR/mise-args"
mkdir -p "$MISE_INSTALLS_DIR/op/2.40.0"
printf '#!/bin/sh\necho NEW\n' > "$MISE_INSTALLS_DIR/op/2.40.0/op"
chmod 755 "$MISE_INSTALLS_DIR/op/2.40.0/op"
exit 0
"#,
    );
    git_init(dir.path());
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^terraform:\n  TF_VAR_FOO: op://v/i/f\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.lock"),
        "[[tools.op]]\nversion = \"2.31.1\"\nbackend = \"aqua:1password/op\"\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();
            let result = block_on(setup_pins(PinMode::Update));
            std::env::set_current_dir(prev).unwrap();
            result.unwrap();
            let lock = std::fs::read_to_string(dir.path().join("lade.lock")).unwrap();
            assert!(lock.contains("2.40.0"), "{lock}");
            assert!(!lock.contains("2.31.1"), "{lock}");
            assert!(installs.join("op/2.40.0/op").is_file());
        },
    );
}
