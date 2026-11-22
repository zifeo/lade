use super::super::ensure;
use super::*;

#[cfg(unix)]
#[test]
fn require_for_inject_refuses_out_of_range_without_fetch() {
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
  printf '%s\n' "mise 2023.1.0"
  exit 0
fi
printf '%s\n' fetched > "$MISE_INSTALLS_DIR/fetched"
exit 1
"#,
    );
    let path = format!("{}:/usr/bin:/bin", stub.display());
    let data = dir.path().join("mise-data");
    let mise = stub.join("mise");
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("LADE_MISE", Some(mise.to_str().unwrap())),
            ("MISE_DATA_DIR", Some(data.to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let err = block_on(ensure::require_for_inject()).unwrap_err();
            assert!(err.to_string().contains("lade setup"), "{err}");
            assert!(!installs.join("fetched").exists());
        },
    );
}

#[cfg(unix)]
#[test]
fn ensure_for_setup_uses_in_range_path_mise_without_fetch() {
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
printf '%s\n' fetched > "$MISE_INSTALLS_DIR/fetched"
exit 1
"#,
    );
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    let mise = stub.join("mise");
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("LADE_MISE", Some(mise.to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir.path()).unwrap();
            let result = block_on(ensure::ensure_for_setup());
            std::env::set_current_dir(prev).unwrap();
            result.unwrap();
            assert!(!installs.join("fetched").exists());
        },
    );
}

#[cfg(unix)]
fn out_of_range_mise(dir: &std::path::Path) -> std::path::PathBuf {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(
        &bin.join("mise"),
        r#"
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2023.1.0"
  exit 0
fi
exit 1
"#,
    );
    bin.join("mise")
}

#[cfg(unix)]
fn in_dir<T>(dir: &std::path::Path, f: impl FnOnce() -> T) -> T {
    let prev = std::env::current_dir().unwrap();
    std::env::set_current_dir(dir).unwrap();
    let out = f();
    std::env::set_current_dir(prev).unwrap();
    out
}

#[cfg(unix)]
#[test]
fn setup_fails_with_install_hint_when_fetch_is_off() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let data = dir.path().join("mise-data");
    let mise = out_of_range_mise(&dir.path().join("stub"));
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", mise.parent().unwrap().display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("LADE_MISE", None),
            ("MISE_DATA_DIR", Some(data.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let err = in_dir(dir.path(), || block_on(ensure::ensure_for_setup()))
                .unwrap_err()
                .to_string();
            assert!(err.contains("2023.1.0"), "{err}");
            assert!(err.contains("mise.jdx.dev/installing-mise"), "{err}");
        },
    );
}

#[cfg(unix)]
#[test]
fn upgrade_refresh_ignores_the_walk() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let data = dir.path().join("mise-data");
    out_of_range_mise(&data);
    std::fs::write(
        dir.path().join("lade.yaml"),
        ": >=99.0.0\n\"^echo\":\n  KEY: v\n",
    )
    .unwrap();
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("LADE_MISE", None),
            ("MISE_DATA_DIR", Some(data.to_str().unwrap())),
            ("PATH", Some("/usr/bin:/bin")),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            assert!(ensure::managed_mise_in_play());
            in_dir(dir.path(), || block_on(ensure::ensure_for_setup())).unwrap();
            let err = in_dir(dir.path(), || block_on(ensure::ensure_managed()))
                .unwrap_err()
                .to_string();
            assert!(err.contains("LADE_MISE_FETCH=0"), "{err}");
            assert!(!err.contains(">=99.0.0"), "{err}");
        },
    );
}

#[test]
fn managed_bin_is_mise_data_dir() {
    let dir = tempdir().unwrap();
    temp_env::with_var("MISE_DATA_DIR", Some(dir.path()), || {
        assert_eq!(ensure::managed_bin(), dir.path().join("bin/mise"));
    });
}

#[cfg(unix)]
#[test]
fn pin_exact_resolves_latest_through_mise() {
    let dir = tempdir().unwrap();
    let stub = dir.path().join("stub");
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), isolation_record_stub());
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("LADE_MISE", Some(stub.join("mise").to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let pinned = pin_exact("mise://aqua/jqlang/jq@latest").unwrap();
            assert_eq!(pinned, "mise://aqua/jqlang/jq@1.7.1");
            let exact = pin_exact("mise://aqua/jqlang/jq@1.8.2").unwrap();
            assert_eq!(exact, "mise://aqua/jqlang/jq@1.8.2");
            let pkg = pin_exact("apm://github/destructure-command-hook").unwrap();
            assert_eq!(pkg, "apm://github/destructure-command-hook");
        },
    );
}

#[cfg(unix)]
#[test]
fn a_mise_query_ignores_the_users_mise_config() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let stub = dir.path().join("stub");
    let record = dir.path().join("record");
    std::fs::create_dir_all(&stub).unwrap();
    std::fs::create_dir_all(home.path().join(".config/mise")).unwrap();
    std::fs::write(home.path().join(".config/mise/config.toml"), "[tools]\n").unwrap();
    std::fs::write(home.path().join(".tool-versions"), "jq 1.6\n").unwrap();
    write_exec(
        &stub.join("mise"),
        &format!(
            r#"
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2024.8.12"
  exit 0
fi
printf '%s|%s|%s\n' "$MISE_IGNORED_CONFIG_PATHS" "$MISE_ENV" "$MISE_GLOBAL_CONFIG_FILE" > "{}"
printf '%s\n' "1.7.1"
"#,
            record.display()
        ),
    );
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("LADE_MISE", Some(stub.join("mise").to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
            ("MISE_ENV", Some("leak")),
            ("MISE_GLOBAL_CONFIG_FILE", Some("/tmp/leak.toml")),
        ],
        || {
            let pinned = pin_exact("mise://aqua/jqlang/jq@latest").unwrap();
            assert_eq!(pinned, "mise://aqua/jqlang/jq@1.7.1");
            let seen = std::fs::read_to_string(&record).unwrap();
            let (ignored, rest) = seen.trim_end().split_once('|').unwrap();
            assert!(
                ignored.contains(&home.path().join(".config/mise").display().to_string()),
                "{seen}"
            );
            assert!(
                ignored.contains(&home.path().join(".tool-versions").display().to_string()),
                "{seen}"
            );
            assert_eq!(rest, "|", "{seen}");
        },
    );
}

#[cfg(unix)]
#[test]
fn an_out_of_range_mise_is_never_run() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let stub = dir.path().join("stub");
    let record = dir.path().join("ran");
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(
        &stub.join("mise"),
        &format!(
            r#"
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2023.1.0"
  exit 0
fi
printf '%s\n' "$*" > "{}"
printf '%s\n' "1.7.1"
"#,
            record.display()
        ),
    );
    let path = format!("{}:/usr/bin:/bin", stub.display());
    let data = dir.path().join("mise-data");
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("LADE_MISE", Some(stub.join("mise").to_str().unwrap())),
            ("MISE_DATA_DIR", Some(data.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let err = pin_exact("mise://aqua/jqlang/jq@latest").unwrap_err();
            assert!(format!("{err:#}").contains("2023.1.0"), "{err:#}");
            assert!(!record.exists());
        },
    );
}
