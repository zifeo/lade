use super::super::ensure;
use super::*;

#[cfg(unix)]
#[test]
fn require_for_inject_refuses_out_of_range_without_fetch() {
    let dir = tempdir().unwrap();
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
    temp_env::with_vars(
        [
            ("LADE_MISE", Some(stub.join("mise").to_str().unwrap())),
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
