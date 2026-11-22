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
            assert!(lock.contains("1.7.1"), "{lock}");
            assert!(!lock.contains("1.8.0"), "{lock}");
            assert!(lock.contains("checksum = \"sha256:"), "{lock}");
            assert!(lock.contains("url = "), "{lock}");
            let args = std::fs::read_to_string(installs.join("mise-args")).unwrap();
            assert!(args.contains("lock"), "{args}");
            assert!(!args.contains("--upgrade"), "{args}");
            assert!(args.contains("install"), "{args}");
            assert!(args.contains("--locked"), "{args}");
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
cd_dir="."
if [ "$1" = "--cd" ]; then
  cd_dir="$2"
  shift 2
fi
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2024.8.12"
  exit 0
fi
if [ "$1" = "latest" ] || [ "$1" = "ls-remote" ]; then
  printf '%s\n' "2.31.1"
  exit 0
fi
printf '%s\n' "$*" >> "$MISE_INSTALLS_DIR/mise-args"
if [ "$1" = "lock" ]; then
  toml="$cd_dir/mise.toml"
  lock="$cd_dir/mise.lock"
  if [ -f "$toml" ]; then
    {
      printf '%s\n' '# @generated'
      printf '%s\n' ''
      printf '%s\n' 'lockfile_version = 2'
      printf '%s\n' ''
      awk '
        BEGIN { in_tools=0 }
        /^\[tools\]/ { in_tools=1; next }
        /^\[/ { in_tools=0 }
        in_tools && /=/ {
          line=$0
          sub(/^[[:space:]]+/, "", line)
          split(line, parts, "=")
          key=parts[1]
          ver=parts[2]
          sub(/^[[:space:]]+/, "", key)
          sub(/[[:space:]]+$/, "", key)
          sub(/^[[:space:]]+/, "", ver)
          sub(/[[:space:]]+$/, "", ver)
          gsub(/^"/, "", key)
          gsub(/"$/, "", key)
          gsub(/^"/, "", ver)
          gsub(/"$/, "", ver)
          printf "[[tools.\"%s\"]]\n", key
          printf "version = \"%s\"\n", ver
          printf "backend = \"%s\"\n", key
          printf "checksum = \"sha256:deadbeef\"\n"
          printf "\n"
          printf "[tools.\"%s\".\"platforms.macos-arm64\"]\n", key
          printf "url = \"https://example.com/%s\"\n", ver
          printf "\n"
        }
      ' "$toml"
    } > "$lock"
  fi
  exit 0
fi
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
            assert!(lock.contains("2.31.1"), "{lock}");
            assert!(lock.contains("checksum = \"sha256:"), "{lock}");
            assert!(lock.contains("url = "), "{lock}");
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
fn setup_and_update_rewrite_latest_with_a_lock_entry() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), isolation_record_stub());
    git_init(dir.path());
    let yaml = dir.path().join("lade.yaml");
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            for (mode, want) in [(PinMode::Locked, "@1.7.0"), (PinMode::Update, "@1.7.1")] {
                std::fs::write(&yaml, "^jq:\n  jq: mise://aqua/jqlang/jq@latest\n").unwrap();
                std::fs::write(
                    dir.path().join("lade.lock"),
                    "[[tools.jq]]\nversion = \"1.7.0\"\nbackend = \"aqua:jqlang/jq\"\n",
                )
                .unwrap();
                let prev = std::env::current_dir().unwrap();
                std::env::set_current_dir(dir.path()).unwrap();
                let result = block_on(setup_pins(mode));
                std::env::set_current_dir(prev).unwrap();
                result.unwrap();
                let text = std::fs::read_to_string(&yaml).unwrap();
                assert!(text.contains(want), "{mode:?}: {text}");
                assert!(!text.contains("@latest"), "{mode:?}: {text}");
            }
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
cd_dir="."
if [ "$1" = "--cd" ]; then
  cd_dir="$2"
  shift 2
fi
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2024.8.12"
  exit 0
fi
printf '%s\n' "$*" >> "$MISE_INSTALLS_DIR/mise-args"
if [ "$1" = "lock" ]; then
  toml="$cd_dir/mise.toml"
  lock="$cd_dir/mise.lock"
  if [ -f "$toml" ]; then
    {
      printf '%s\n' '# @generated'
      printf '%s\n' ''
      printf '%s\n' 'lockfile_version = 2'
      printf '%s\n' ''
      awk '
        BEGIN { in_tools=0 }
        /^\[tools\]/ { in_tools=1; next }
        /^\[/ { in_tools=0 }
        in_tools && /=/ {
          line=$0
          sub(/^[[:space:]]+/, "", line)
          split(line, parts, "=")
          key=parts[1]
          ver=parts[2]
          sub(/^[[:space:]]+/, "", key)
          sub(/[[:space:]]+$/, "", key)
          sub(/^[[:space:]]+/, "", ver)
          sub(/[[:space:]]+$/, "", ver)
          gsub(/^"/, "", key)
          gsub(/"$/, "", key)
          gsub(/^"/, "", ver)
          gsub(/"$/, "", ver)
          printf "[[tools.\"%s\"]]\n", key
          printf "version = \"%s\"\n", ver
          printf "backend = \"%s\"\n", key
          printf "checksum = \"sha256:deadbeef\"\n"
          printf "\n"
          printf "[tools.\"%s\".\"platforms.macos-arm64\"]\n", key
          printf "url = \"https://example.com/%s\"\n", ver
          printf "\n"
        }
      ' "$toml"
    } > "$lock"
  fi
  exit 0
fi
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
            assert!(lock.contains("op") || lock.contains("1password"), "{lock}");
            assert!(lock.contains("jq"), "{lock}");
            assert!(!child.join("lade.lock").exists());
        },
    );
}

#[test]
fn setup_warns_about_a_leftover_lade_lock() {
    let home = tempdir().unwrap();
    let proj = home.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join("mise.lock"), "lockfile_version = 1\n").unwrap();
    std::fs::write(proj.join("lade.lock"), "lockfile_version = 1\n").unwrap();
    temp_env::with_var("HOME", Some(home.path()), || {
        let lines = super::super::setup::plane_warnings(&crate::mise::scan(&proj));
        assert_eq!(
            lines,
            vec![format!(
                "Leftover {} is ignored.",
                proj.join("lade.lock").display()
            )]
        );
    });
}

#[test]
fn setup_warns_when_toml_and_lock_are_split() {
    let home = tempdir().unwrap();
    let proj = home.path().join("proj");
    let app = proj.join("app");
    std::fs::create_dir_all(&app).unwrap();
    std::fs::write(proj.join("mise.toml"), "[tools]\n").unwrap();
    std::fs::write(app.join("mise.lock"), "lockfile_version = 1\n").unwrap();
    temp_env::with_var("HOME", Some(home.path()), || {
        let lines = super::super::setup::plane_warnings(&crate::mise::scan(&app));
        assert!(
            lines
                .iter()
                .any(|line| line.contains("different directories")),
            "{lines:?}"
        );
        assert!(
            lines.iter().any(|line| line.contains("left as-is")),
            "{lines:?}"
        );
    });
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
            assert!(!args.contains("--upgrade"), "{args}");
        },
    );
}

#[cfg(unix)]
#[test]
fn setup_runs_the_pin_setup_command_at_the_git_root() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("dcg/1.0.0");
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(
        &bin.join("dcg"),
        r#"printf '%s|%s\n' "$(pwd -P)" "$*" >> "$MISE_INSTALLS_DIR/dcg-ran""#,
    );
    git_init(dir.path());
    let sub = dir.path().join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        dir.path().join("lade.yaml"),
        ".:\n  dcg: \"mise://github/owner/dcg@1.0.0?setup=install --yes\"\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
        ],
        || {
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(&sub).unwrap();
            let result = block_on(run_lifecycle_commands("setup"));
            std::env::set_current_dir(prev).unwrap();
            result.unwrap();
        },
    );
    let ran = std::fs::read_to_string(installs.join("dcg-ran")).unwrap();
    let root = dir.path().canonicalize().unwrap();
    assert_eq!(ran.trim(), format!("{}|install --yes", root.display()));
}

#[cfg(unix)]
#[test]
fn unlock_ignores_a_fitting_lock_and_rewrites_it() {
    let run = |mode: PinMode| {
        let dir = tempdir().unwrap();
        let home = tempdir().unwrap();
        let installs = dir.path().join("installs");
        let stub = dir.path().join("stub");
        std::fs::create_dir_all(&stub).unwrap();
        for version in ["1.7.0", "1.7.1"] {
            std::fs::create_dir_all(installs.join("jq").join(version)).unwrap();
            write_exec(&installs.join("jq").join(version).join("jq"), "echo JQ");
        }
        write_exec(&stub.join("mise"), isolation_record_stub());
        git_init(dir.path());
        std::fs::write(
            dir.path().join("lade.yaml"),
            "^jq:\n  jq: mise://aqua/jqlang/jq@>=1.7.0\n",
        )
        .unwrap();
        std::fs::write(
            dir.path().join("lade.lock"),
            "[[tools.jq]]\nversion = \"1.7.0\"\nbackend = \"aqua:jqlang/jq\"\n",
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
                let result = block_on(setup_pins(mode));
                std::env::set_current_dir(prev).unwrap();
                result.unwrap();
                let lock = std::fs::read_to_string(dir.path().join("lade.lock")).unwrap();
                let args = std::fs::read_to_string(installs.join("mise-args")).unwrap_or_default();
                (lock, args)
            },
        )
    };
    let (lock, _) = run(PinMode::Locked);
    assert!(lock.contains("\"1.7.0\""), "{lock}");
    let (lock, args) = run(PinMode::Unlock);
    assert!(lock.contains("\"1.7.1\""), "{lock}");
    assert!(!lock.contains("\"1.7.0\""), "{lock}");
    assert!(args.lines().any(|line| line.contains("lock")), "{args}");
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
cd_dir="."
if [ "$1" = "--cd" ]; then
  cd_dir="$2"
  shift 2
fi
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2024.8.12"
  exit 0
fi
if [ "$1" = "latest" ] || [ "$1" = "ls-remote" ]; then
  printf '%s\n' "2.40.0"
  exit 0
fi
printf '%s\n' "$*" >> "$MISE_INSTALLS_DIR/mise-args"
if [ "$1" = "lock" ]; then
  toml="$cd_dir/mise.toml"
  lock="$cd_dir/mise.lock"
  if [ -f "$toml" ]; then
    {
      printf '%s\n' '# @generated'
      printf '%s\n' ''
      printf '%s\n' 'lockfile_version = 2'
      printf '%s\n' ''
      awk '
        BEGIN { in_tools=0 }
        /^\[tools\]/ { in_tools=1; next }
        /^\[/ { in_tools=0 }
        in_tools && /=/ {
          line=$0
          sub(/^[[:space:]]+/, "", line)
          split(line, parts, "=")
          key=parts[1]
          ver=parts[2]
          sub(/^[[:space:]]+/, "", key)
          sub(/[[:space:]]+$/, "", key)
          sub(/^[[:space:]]+/, "", ver)
          sub(/[[:space:]]+$/, "", ver)
          gsub(/^"/, "", key)
          gsub(/"$/, "", key)
          gsub(/^"/, "", ver)
          gsub(/"$/, "", ver)
          printf "[[tools.\"%s\"]]\n", key
          printf "version = \"%s\"\n", ver
          printf "backend = \"%s\"\n", key
          printf "checksum = \"sha256:deadbeef\"\n"
          printf "\n"
          printf "[tools.\"%s\".\"platforms.macos-arm64\"]\n", key
          printf "url = \"https://example.com/%s\"\n", ver
          printf "\n"
        }
      ' "$toml"
    } > "$lock"
  fi
  exit 0
fi
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
        "[[tools.op]]\nversion = \"2.31.1\"\nbackend = \"aqua:1password/cli\"\n",
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

#[cfg(unix)]
#[test]
fn setup_implied_op_when_latest_prints_nothing() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), range_rejecting_stub());
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
            assert!(lock.contains("2.39.0"), "{lock}");
            assert!(!lock.contains(">="), "{lock}");
            let args = std::fs::read_to_string(installs.join("mise-args")).unwrap();
            assert!(!args.contains(">="), "{args}");
        },
    );
}

#[cfg(unix)]
#[test]
fn setup_refuses_a_range_when_nothing_lists_it() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), range_rejecting_stub());
    git_init(dir.path());
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@>=1.7.0\n",
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
            let text = result.unwrap_err().to_string();
            assert!(text.contains("aqua:jqlang/jq"), "{text}");
            assert!(text.contains("not installed"), "{text}");
            assert!(!dir.path().join("lade.lock").exists());
            if installs.join("mise-args").is_file() {
                let args = std::fs::read_to_string(installs.join("mise-args")).unwrap();
                assert!(!args.contains("install"), "{args}");
            }
        },
    );
}

#[cfg(unix)]
#[test]
fn update_keeps_a_newer_lock_when_latest_prints_nothing() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), range_rejecting_stub());
    git_init(dir.path());
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^terraform:\n  TF_VAR_FOO: op://v/i/f\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.lock"),
        "[[tools.op]]\nversion = \"2.40.0\"\nbackend = \"aqua:1password/cli\"\n",
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
            assert!(!lock.contains("2.39.0"), "{lock}");
            assert!(!lock.contains(">="), "{lock}");
        },
    );
}

#[cfg(unix)]
fn range_rejecting_stub() -> &'static str {
    r#"
cd_dir="."
if [ "$1" = "--cd" ]; then
  cd_dir="$2"
  shift 2
fi
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2024.8.12"
  exit 0
fi
if [ "$1" = "latest" ] || [ "$1" = "ls-remote" ]; then
  exit 0
fi
printf '%s\n' "$*" >> "$MISE_INSTALLS_DIR/mise-args"
if printf '%s' "$*" | grep -q '>='; then
  printf '%s\n' "range on argv" >&2
  exit 1
fi
if [ -f "$cd_dir/mise.toml" ] && grep -q '>=' "$cd_dir/mise.toml"; then
  printf '%s\n' "range in mise.toml" >&2
  exit 1
fi
if [ "$1" = "lock" ]; then
  cat "$cd_dir/mise.toml" > "$cd_dir/mise.lock"
  exit 0
fi
exit 0
"#
}
