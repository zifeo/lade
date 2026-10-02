use super::*;

const INFISICAL_SOURCE: &str = "^app:\n  TOKEN: infisical://app.infisical.com/project/dev/TOKEN\n";

fn provider_pin(version: &str, pattern: &str) -> String {
    format!(
        "^infisical:\n  infisical: mise://github/Infisical/cli[asset_pattern={pattern}]@{version}\n"
    )
}

fn implied_infisical_pin() -> String {
    let uri = super::super::implied::by_key("infisical")
        .unwrap()
        .uri
        .clone();
    format!("^infisical:\n  infisical: {uri}\n")
}

fn run_setup(
    dir: &std::path::Path,
    home: &std::path::Path,
    installs: &std::path::Path,
    mode: PinMode,
) {
    let stub = dir.join("stub");
    std::fs::create_dir_all(installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), isolation_record_stub());
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
            ("LADE_MISE_FETCH", Some("0")),
        ],
        || {
            let prev = std::env::current_dir().unwrap();
            std::env::set_current_dir(dir).unwrap();
            let result = block_on(setup_pins(mode));
            std::env::set_current_dir(prev).unwrap();
            result.unwrap();
        },
    );
}

fn isolated(installs: &std::path::Path) -> String {
    std::fs::read_to_string(installs.join("isolated.toml")).unwrap()
}

fn args(installs: &std::path::Path) -> String {
    std::fs::read_to_string(installs.join("mise-args")).unwrap()
}

#[cfg(unix)]
#[test]
fn normal_mise_alias_is_installed_as_declared() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    let toml = "[tools]\nnode = \"24\"\ninfisical = \"0.43.55\"\n";
    let lock = "[[tools.infisical]]\nversion = \"0.43.55\"\nbackend = \"github:Infisical/cli\"\n";
    std::fs::write(dir.path().join("mise.toml"), toml).unwrap();
    std::fs::write(dir.path().join("mise.lock"), lock).unwrap();
    std::fs::write(dir.path().join("lade.yml"), INFISICAL_SOURCE).unwrap();
    run_setup(dir.path(), home.path(), &installs, PinMode::Locked);
    let body = isolated(&installs);
    assert!(body.contains("infisical = \"0.43.55\""), "{body}");
    assert!(!body.contains("asset_pattern"), "{body}");
    assert!(!body.contains("github:Infisical/cli"), "{body}");
    let ran = args(&installs);
    assert!(ran.contains("--locked"), "{ran}");
    assert!(ran.contains("infisical"), "{ran}");
    assert!(!ran.contains("github:Infisical/cli"), "{ran}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mise.lock")).unwrap(),
        lock
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mise.toml")).unwrap(),
        toml
    );
}

#[cfg(unix)]
#[test]
fn normal_mise_backend_pin_is_installed_without_implied_options() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    let toml = "[tools]\n\"github:Infisical/cli\" = \"0.43.55\"\n";
    let lock = "[[tools.\"github:Infisical/cli\"]]\nversion = \"0.43.55\"\nbackend = \"github:Infisical/cli\"\n";
    std::fs::write(dir.path().join("mise.toml"), toml).unwrap();
    std::fs::write(dir.path().join("mise.lock"), lock).unwrap();
    std::fs::write(dir.path().join("lade.yml"), INFISICAL_SOURCE).unwrap();
    run_setup(dir.path(), home.path(), &installs, PinMode::Locked);
    let body = isolated(&installs);
    assert!(body.contains("github:Infisical/cli"), "{body}");
    assert!(body.contains("0.43.55"), "{body}");
    assert!(!body.contains("asset_pattern"), "{body}");
    assert!(!body.contains("\ninfisical ="), "{body}");
    let ran = args(&installs);
    assert!(ran.contains("--locked"), "{ran}");
    assert!(ran.contains("github:Infisical/cli"), "{ran}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mise.lock")).unwrap(),
        lock
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mise.toml")).unwrap(),
        toml
    );
}

#[cfg(unix)]
#[test]
fn normal_mise_declared_options_are_kept() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\n\"github:Infisical/cli\" = { version = \"0.43.55\", asset_pattern = \"from-toml\" }\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("mise.lock"),
        "[[tools.\"github:Infisical/cli\"]]\nversion = \"0.43.55\"\nbackend = \"github:Infisical/cli\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("lade.yml"), INFISICAL_SOURCE).unwrap();
    run_setup(dir.path(), home.path(), &installs, PinMode::Locked);
    let body = isolated(&installs);
    assert!(body.contains("from-toml"), "{body}");
    assert!(!body.contains("macos"), "{body}");
    let written = std::fs::read_to_string(dir.path().join("mise.toml")).unwrap();
    assert!(written.contains("from-toml"), "{written}");
}

#[cfg(unix)]
#[test]
fn missing_tool_is_added_by_mise_use() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    std::fs::write(dir.path().join("mise.toml"), "[tools]\nnode = \"24\"\n").unwrap();
    std::fs::write(dir.path().join("lade.yml"), INFISICAL_SOURCE).unwrap();
    run_setup(dir.path(), home.path(), &installs, PinMode::Locked);
    let ran = args(&installs);
    assert!(ran.contains("use"), "{ran}");
    assert!(ran.contains("--path"), "{ran}");
    assert!(ran.contains("github:Infisical/cli@1.7.1"), "{ran}");
    let written = std::fs::read_to_string(dir.path().join("mise.toml")).unwrap();
    assert!(written.contains("node = \"24\""), "{written}");
    assert!(written.contains("written by mise use"), "{written}");
    assert!(written.contains("github:Infisical/cli"), "{written}");
    assert!(written.contains("1.7.1"), "{written}");
    assert!(!written.contains("asset_pattern"), "{written}");
    let body = isolated(&installs);
    assert!(body.contains("github:Infisical/cli"), "{body}");
    assert!(body.contains("1.7.1"), "{body}");
    assert!(!body.contains("asset_pattern"), "{body}");
    assert!(dir.path().join("mise.lock").is_file());
    assert!(!dir.path().join("lade.lock").exists());
}

#[cfg(unix)]
#[test]
fn leaf_provider_pin_wins_over_the_parent() {
    let root = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = root.path().join("installs");
    let child = root.path().join("child");
    git_init(root.path());
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(
        root.path().join("lade.yml"),
        provider_pin("1.0.0", "from-parent"),
    )
    .unwrap();
    std::fs::write(child.join("lade.yml"), provider_pin("9.9.9", "from-leaf")).unwrap();
    run_setup(&child, home.path(), &installs, PinMode::Locked);
    let body = isolated(&installs);
    assert!(body.contains("9.9.9"), "{body}");
    assert!(body.contains("from-leaf"), "{body}");
    assert!(!body.contains("1.0.0"), "{body}");
    assert!(!body.contains("from-parent"), "{body}");
    assert!(root.path().join("lade.lock").is_file());
    assert!(!child.join("lade.lock").exists());
}

#[cfg(unix)]
#[test]
fn leaf_provider_pin_wins_over_parent_and_mise_toml() {
    let root = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = root.path().join("installs");
    let child = root.path().join("child");
    git_init(root.path());
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(
        root.path().join("mise.toml"),
        "[tools]\ninfisical = \"0.43.55\"\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("lade.yml"),
        provider_pin("1.0.0", "from-parent"),
    )
    .unwrap();
    std::fs::write(child.join("lade.yml"), provider_pin("8.8.8", "from-leaf")).unwrap();
    run_setup(&child, home.path(), &installs, PinMode::Locked);
    let body = isolated(&installs);
    assert!(body.contains("8.8.8"), "{body}");
    assert!(body.contains("from-leaf"), "{body}");
    assert!(!body.contains("0.43.55"), "{body}");
    assert!(!body.contains("1.0.0"), "{body}");
    let written = std::fs::read_to_string(root.path().join("mise.toml")).unwrap();
    assert!(written.contains("8.8.8"), "{written}");
    assert!(written.contains("github:Infisical/cli"), "{written}");
    assert!(!written.contains("0.43.55"), "{written}");
}

#[cfg(unix)]
#[test]
fn provider_pin_wins_over_mise_toml() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\ninfisical = \"0.43.55\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        provider_pin("9.9.9", "from-yaml"),
    )
    .unwrap();
    run_setup(dir.path(), home.path(), &installs, PinMode::Locked);
    let body = isolated(&installs);
    assert!(body.contains("9.9.9"), "{body}");
    assert!(body.contains("from-yaml"), "{body}");
    assert!(!body.contains("0.43.55"), "{body}");
    let written = std::fs::read_to_string(dir.path().join("mise.toml")).unwrap();
    assert!(written.contains("9.9.9"), "{written}");
    assert!(written.contains("github:Infisical/cli"), "{written}");
    assert!(!written.contains("0.43.55"), "{written}");
}

#[cfg(unix)]
#[test]
fn provider_range_resolves_without_the_toml_version() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\ninfisical = \"0.43.55\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        provider_pin(">=0.4.0", "from-yaml"),
    )
    .unwrap();
    run_setup(dir.path(), home.path(), &installs, PinMode::Locked);
    let body = isolated(&installs);
    assert!(body.contains("1.7.1"), "{body}");
    assert!(body.contains("from-yaml"), "{body}");
    assert!(!body.contains("0.43.55"), "{body}");
}

#[cfg(unix)]
#[test]
fn two_part_row_keeps_the_padded_lock() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    let toml = "[tools]\ninfisical = \"0.43\"\n";
    let lock = "[[tools.infisical]]\nversion = \"0.43.55\"\nbackend = \"github:Infisical/cli\"\n";
    std::fs::write(dir.path().join("mise.toml"), toml).unwrap();
    std::fs::write(dir.path().join("mise.lock"), lock).unwrap();
    std::fs::write(dir.path().join("lade.yml"), INFISICAL_SOURCE).unwrap();
    run_setup(dir.path(), home.path(), &installs, PinMode::Locked);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mise.toml")).unwrap(),
        toml
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mise.lock")).unwrap(),
        lock
    );
    let ran = args(&installs);
    assert!(ran.contains("--locked"), "{ran}");
    assert!(ran.contains("infisical"), "{ran}");
    assert!(!ran.contains("github:Infisical/cli"), "{ran}");
    let body = isolated(&installs);
    assert!(body.contains("infisical = \"0.43.55\""), "{body}");
}

#[cfg(unix)]
#[test]
fn out_of_range_alias_stays_when_a_backend_row_fits() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    let toml = "[tools]\ninfisical = \"0.1.0\"\n\"github:Infisical/cli\" = \"0.43.55\"\n";
    let lock = "[[tools.\"github:Infisical/cli\"]]\nversion = \"0.43.55\"\nbackend = \"github:Infisical/cli\"\n";
    std::fs::write(dir.path().join("mise.toml"), toml).unwrap();
    std::fs::write(dir.path().join("mise.lock"), lock).unwrap();
    std::fs::write(dir.path().join("lade.yml"), INFISICAL_SOURCE).unwrap();
    run_setup(dir.path(), home.path(), &installs, PinMode::Locked);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mise.toml")).unwrap(),
        toml
    );
    run_setup(dir.path(), home.path(), &installs, PinMode::Unlock);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mise.toml")).unwrap(),
        toml
    );
    let ran = args(&installs);
    assert!(ran.contains("--locked"), "{ran}");
    assert!(ran.contains("github:Infisical/cli"), "{ran}");
}

#[cfg(unix)]
#[test]
fn adding_mise_toml_uses_its_row_and_leaves_the_old_lock() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    let old = "[[tools.\"github:Infisical/cli\"]]\nversion = \"0.43.55\"\nbackend = \"github:Infisical/cli\"\n";
    std::fs::write(dir.path().join("lade.lock"), old).unwrap();
    let toml = "[tools]\ninfisical = \"0.50.0\"\n";
    std::fs::write(dir.path().join("mise.toml"), toml).unwrap();
    std::fs::write(dir.path().join("lade.yml"), INFISICAL_SOURCE).unwrap();
    run_setup(dir.path(), home.path(), &installs, PinMode::Locked);
    let body = isolated(&installs);
    assert!(body.contains("infisical = \"0.50.0\""), "{body}");
    assert!(!body.contains("asset_pattern"), "{body}");
    assert!(!body.contains("0.43.55"), "{body}");
    let ran = args(&installs);
    assert!(ran.contains("--locked"), "{ran}");
    assert!(ran.contains("infisical"), "{ran}");
    assert!(!ran.contains("use --path"), "{ran}");
    assert!(!ran.contains("github:Infisical/cli"), "{ran}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mise.toml")).unwrap(),
        toml
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("lade.lock")).unwrap(),
        old
    );
    let lock = std::fs::read_to_string(dir.path().join("mise.lock")).unwrap();
    assert!(lock.contains("0.50.0"), "{lock}");
}

#[cfg(unix)]
#[test]
fn adding_mise_toml_without_the_tool_asks_mise_to_write_it() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    let old = "[[tools.\"github:Infisical/cli\"]]\nversion = \"0.43.55\"\nbackend = \"github:Infisical/cli\"\n";
    std::fs::write(dir.path().join("lade.lock"), old).unwrap();
    std::fs::write(dir.path().join("mise.toml"), "[tools]\nnode = \"24\"\n").unwrap();
    std::fs::write(dir.path().join("lade.yml"), INFISICAL_SOURCE).unwrap();
    run_setup(dir.path(), home.path(), &installs, PinMode::Locked);
    let ran = args(&installs);
    assert!(ran.contains("use"), "{ran}");
    assert!(ran.contains("--path"), "{ran}");
    assert!(
        ran.contains(dir.path().join("mise.toml").to_str().unwrap()),
        "{ran}"
    );
    assert!(ran.contains("github:Infisical/cli@1.7.1"), "{ran}");
    let written = std::fs::read_to_string(dir.path().join("mise.toml")).unwrap();
    assert!(written.contains("node = \"24\""), "{written}");
    assert!(written.contains("written by mise use"), "{written}");
    assert!(written.contains("github:Infisical/cli"), "{written}");
    assert!(written.contains("1.7.1"), "{written}");
    assert!(!written.contains("asset_pattern"), "{written}");
    assert!(!written.contains("0.43.55"), "{written}");
    let body = isolated(&installs);
    assert!(body.contains("github:Infisical/cli"), "{body}");
    assert!(body.contains("1.7.1"), "{body}");
    assert!(!body.contains("asset_pattern"), "{body}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("lade.lock")).unwrap(),
        old
    );
    let lock = std::fs::read_to_string(dir.path().join("mise.lock")).unwrap();
    assert!(lock.contains("1.7.1"), "{lock}");
}

#[cfg(unix)]
#[test]
fn lade_plane_uses_the_implied_spec_and_ignores_home() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    std::fs::write(
        home.path().join("mise.toml"),
        "[tools]\ninfisical = \"9.9.9\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.lock"),
        "[[tools.\"github:Infisical/cli\"]]\nversion = \"0.43.55\"\nbackend = \"github:Infisical/cli\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("lade.yml"), INFISICAL_SOURCE).unwrap();
    run_setup(dir.path(), home.path(), &installs, PinMode::Locked);
    let body = isolated(&installs);
    assert!(body.contains("github:Infisical/cli"), "{body}");
    assert!(body.contains("0.43.55"), "{body}");
    assert!(body.contains("asset_pattern"), "{body}");
    assert!(!body.contains("9.9.9"), "{body}");
    assert!(!body.contains("infisical ="), "{body}");
    assert!(!dir.path().join("mise.toml").exists());
    let lock = std::fs::read_to_string(dir.path().join("lade.lock")).unwrap();
    assert!(lock.contains("0.43.55"), "{lock}");
    assert!(!lock.contains("9.9.9"), "{lock}");
}

#[cfg(unix)]
#[test]
fn normal_mise_update_keeps_the_declared_alias() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\ninfisical = \"0.43.55\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("mise.lock"),
        "[[tools.infisical]]\nversion = \"0.43.55\"\nbackend = \"github:Infisical/cli\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("lade.yml"), INFISICAL_SOURCE).unwrap();
    run_setup(dir.path(), home.path(), &installs, PinMode::Update);
    let written = std::fs::read_to_string(dir.path().join("mise.toml")).unwrap();
    assert!(written.contains("infisical = \"1.7.1\""), "{written}");
    assert!(!written.contains("github:Infisical/cli"), "{written}");
    assert!(!written.contains("asset_pattern"), "{written}");
    let body = isolated(&installs);
    assert!(body.contains("infisical = \"1.7.1\""), "{body}");
    assert!(!body.contains("asset_pattern"), "{body}");
    let lock = std::fs::read_to_string(dir.path().join("mise.lock")).unwrap();
    assert!(lock.contains("1.7.1"), "{lock}");
    assert!(
        lock.contains("tools.\"infisical\"") || lock.contains("tools.infisical"),
        "{lock}"
    );
    assert!(!lock.contains("asset_pattern"), "{lock}");
}

#[cfg(unix)]
#[test]
fn normal_mise_unlock_keeps_the_declared_alias() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    let toml = "[tools]\ninfisical = \"0.43.55\"\n";
    std::fs::write(dir.path().join("mise.toml"), toml).unwrap();
    std::fs::write(
        dir.path().join("mise.lock"),
        "[[tools.infisical]]\nversion = \"0.43.55\"\nbackend = \"github:Infisical/cli\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("lade.yml"), INFISICAL_SOURCE).unwrap();
    run_setup(dir.path(), home.path(), &installs, PinMode::Unlock);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mise.toml")).unwrap(),
        toml
    );
    let body = isolated(&installs);
    assert!(body.contains("infisical = \"0.43.55\""), "{body}");
    assert!(!body.contains("asset_pattern"), "{body}");
    assert!(!body.contains("github:Infisical/cli"), "{body}");
}

#[cfg(unix)]
fn install_infisical_stub() -> &'static str {
    r#"
if [ "$1" = "--cd" ]; then
  shift 2
fi
if [ "$1" = "--version" ]; then
  printf '%s\n' "mise 2024.8.12"
  exit 0
fi
printf '%s\n' "$*" >> "$MISE_INSTALLS_DIR/mise-args"
if [ -n "$MISE_GLOBAL_CONFIG_FILE" ]; then
  while IFS= read -r line; do
    printf '%s\n' "$line"
  done < "$MISE_GLOBAL_CONFIG_FILE" > "$MISE_INSTALLS_DIR/isolated.toml"
fi
if printf '%s' "$*" | grep -q -- '--json-extended'; then
  printf '%s\n' '{}'
  exit 0
fi
mkdir -p "$MISE_INSTALLS_DIR/infisical/0.43.55"
printf '#!/bin/sh\necho INF\n' > "$MISE_INSTALLS_DIR/infisical/0.43.55/infisical"
chmod 755 "$MISE_INSTALLS_DIR/infisical/0.43.55/infisical"
exit 0
"#
}

#[cfg(unix)]
fn prepare_infisical(
    dir: &std::path::Path,
    home: &std::path::Path,
    installs: &std::path::Path,
    yaml: &str,
) -> std::collections::HashMap<String, String> {
    let stub = dir.join("stub");
    std::fs::create_dir_all(installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), install_infisical_stub());
    std::fs::write(dir.join("lade.yml"), yaml).unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    let mut out = std::collections::HashMap::new();
    temp_env::with_vars(
        [
            ("HOME", Some(home.to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
        ],
        || {
            let config = LadeFile::build(dir.to_path_buf()).unwrap();
            let prepared = block_on(prepare(
                &config,
                "infisical export",
                dir,
                &None,
                Audience::Human,
            ))
            .unwrap();
            out = prepared.env;
        },
    );
    out
}

#[cfg(unix)]
#[test]
fn normal_mise_prepare_uses_the_declared_alias() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\ninfisical = \"0.43.55\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("mise.lock"),
        "lockfile_version = 2\n[[tools.infisical]]\nversion = \"0.43.55\"\nbackend = \"github:Infisical/cli\"\nurl = \"https://example.com/infisical\"\n",
    )
    .unwrap();
    let bin = installs.join("infisical/0.43.55");
    std::fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("infisical"), "echo INF");
    let env = prepare_infisical(
        dir.path(),
        home.path(),
        &installs,
        "^infisical:\n  TOKEN: infisical://app.infisical.com/project/dev/TOKEN\n",
    );
    let path = env.get("PATH").unwrap();
    assert!(
        path.contains(&format!("{}", installs.join("infisical/0.43.55").display())),
        "{path}"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mise.toml")).unwrap(),
        "[tools]\ninfisical = \"0.43.55\"\n"
    );
    if let Ok(ran) = std::fs::read_to_string(installs.join("mise-args")) {
        assert!(!ran.contains("use"), "{ran}");
        assert!(!ran.contains("github:Infisical/cli"), "{ran}");
    }
}

#[cfg(unix)]
#[test]
fn yaml_pin_of_the_builtin_uri_keeps_its_own_spec() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    std::fs::write(
        dir.path().join("mise.toml"),
        "[tools]\ninfisical = \"0.43.55\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("mise.lock"),
        "[[tools.infisical]]\nversion = \"0.43.55\"\nbackend = \"github:Infisical/cli\"\n",
    )
    .unwrap();
    std::fs::write(dir.path().join("lade.yml"), implied_infisical_pin()).unwrap();
    run_setup(dir.path(), home.path(), &installs, PinMode::Locked);
    let body = isolated(&installs);
    assert!(body.contains("asset_pattern"), "{body}");
    assert!(body.contains("0.43.55"), "{body}");
    assert!(body.contains("github:Infisical/cli"), "{body}");
}

#[cfg(unix)]
#[test]
fn prepare_without_a_row_leaves_the_old_lade_lock_unread() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let stub = dir.path().join("stub");
    git_init(dir.path());
    std::fs::create_dir_all(&installs).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    write_exec(&stub.join("mise"), install_infisical_stub());
    let toml = "[tools]\nnode = \"24\"\n";
    std::fs::write(dir.path().join("mise.toml"), toml).unwrap();
    let old = "lockfile_version = 2\n[[tools.\"github:Infisical/cli\"]]\nversion = \"0.43.55\"\nbackend = \"github:Infisical/cli\"\nurl = \"https://example.com/infisical\"\n";
    std::fs::write(dir.path().join("lade.lock"), old).unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "^infisical:\n  TOKEN: infisical://app.infisical.com/project/dev/TOKEN\n",
    )
    .unwrap();
    let path = format!("{}:/usr/bin:/bin", stub.display());
    temp_env::with_vars(
        [
            ("HOME", Some(home.path().to_str().unwrap())),
            ("MISE_INSTALLS_DIR", Some(installs.to_str().unwrap())),
            ("PATH", Some(path.as_str())),
        ],
        || {
            let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
            let prepared = block_on(prepare(
                &config,
                "infisical export",
                dir.path(),
                &None,
                Audience::Human,
            ));
            assert!(prepared.is_err(), "{prepared:?}");
        },
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mise.toml")).unwrap(),
        toml
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("lade.lock")).unwrap(),
        old
    );
    assert!(!installs.join("mise-args").exists());
}

#[cfg(unix)]
#[test]
fn lade_plane_prepare_installs_the_implied_spec() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    git_init(dir.path());
    std::fs::write(
        home.path().join("mise.toml"),
        "[tools]\ninfisical = \"9.9.9\"\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.lock"),
        "lockfile_version = 2\n[[tools.\"github:Infisical/cli\"]]\nversion = \"0.43.55\"\nbackend = \"github:Infisical/cli\"\nurl = \"https://example.com/infisical\"\n",
    )
    .unwrap();
    let env = prepare_infisical(dir.path(), home.path(), &installs, &implied_infisical_pin());
    let path = env.get("PATH").unwrap();
    assert!(path.contains("infisical/0.43.55"), "{path}");
    assert!(!path.contains("9.9.9"), "{path}");
    let body = isolated(&installs);
    assert!(body.contains("github:Infisical/cli"), "{body}");
    assert!(body.contains("asset_pattern"), "{body}");
    assert!(!body.contains("9.9.9"), "{body}");
    assert!(!body.contains("infisical ="), "{body}");
    let ran = args(&installs);
    assert!(ran.contains("--locked"), "{ran}");
    assert!(ran.contains("github:Infisical/cli"), "{ran}");
}
