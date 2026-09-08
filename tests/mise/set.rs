use super::common;
use super::support::*;
use std::fs;
use tempfile::tempdir;

#[cfg(unix)]
#[test]
fn set_mise_ls_ignores_home_java_keeps_project_node() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let tickets = home.path().join("tickets");
    fs::create_dir_all(&tickets).unwrap();
    fs::create_dir_all(home.path().join(".config/mise")).unwrap();
    fs::write(
        home.path().join(".config/mise/config.toml"),
        "[tools]\njava = \"21\"\n[env]\nNODE_VERSION = \"24\"\n",
    )
    .unwrap();
    fs::write(home.path().join("mise.toml"), "[tools]\njava = \"21\"\n").unwrap();
    fs::write(
        dir.path().join("mise.toml"),
        "[tools]\nnode = \"24.16.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "^mise:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let stdout = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", &tickets)
        .args(["set", "mise", "ls"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&stdout);
    let composed_path =
        export_value(&stdout, "MISE_GLOBAL_CONFIG_FILE").expect("composed mise config");
    let composed = fs::read_to_string(composed_path).unwrap();
    assert!(composed.contains("node = \"24.16.0\""), "{composed}");
    assert!(composed.contains("aqua:jqlang/jq"), "{composed}");
    assert!(!composed.contains("java"), "{composed}");
    assert!(!composed.contains("NODE_VERSION"), "{composed}");
    assert!(stdout.contains("export MISE_CONFIG_DIR="), "{stdout}");
    let ignored = export_value(&stdout, "MISE_IGNORED_CONFIG_PATHS").expect("ignored paths");
    assert!(
        ignored.contains(&home.path().join(".config/mise").display().to_string()),
        "{ignored}"
    );
}

#[cfg(unix)]
#[test]
fn set_then_unset_leaves_repo_without_mise_files() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let tickets = home.path().join("tickets");
    fs::create_dir_all(&tickets).unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "^mise:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let set = common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", &tickets)
        .args(["set", "mise", "ls"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&set);
    assert!(
        stdout.contains("MISE_GLOBAL_CONFIG_FILE") || stdout.contains("LADE_MISE_CONFIG"),
        "{stdout}"
    );
    assert!(!dir.path().join("mise.toml").exists());
    common::lade(home.path())
        .current_dir(dir.path())
        .env("LADE_TICKET_DIR", &tickets)
        .env(
            "LADE_MISE_CONFIG",
            tickets
                .read_dir()
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .find(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("lade-mise-") && n.ends_with(".toml"))
                })
                .expect("temp mise file"),
        )
        .args(["unset", "mise", "ls"])
        .assert()
        .success();
    let leftovers: Vec<_> = tickets
        .read_dir()
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_name()
                .to_str()
                .is_some_and(|n| n.starts_with("lade-mise-"))
        })
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    assert!(!dir.path().join("mise.toml").exists());
    assert!(!dir.path().join("mise.lock").exists());
}

#[cfg(unix)]
#[test]
fn set_jq_pin_exports_store_path_first() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("jq/1.7.1");
    fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("jq"), "echo PINNED");
    write_cached_env(
        home.path(),
        "aqua:jqlang/jq",
        "aqua-jqlang-jq",
        "1.7.1",
        "{}",
    );
    fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let stdout = common::lade(home.path())
        .current_dir(dir.path())
        .env("MISE_INSTALLS_DIR", &installs)
        .env("PATH", "/usr/bin")
        .args(["set", "jq", "."])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&stdout);
    let path = export_value(&stdout, "PATH").expect("PATH");
    assert!(
        path.starts_with(&format!("{}/", bin.display()))
            || path.starts_with(&format!("{}:", bin.display())),
        "{path}"
    );
}

#[cfg(unix)]
#[test]
fn unset_restores_path_after_jq_pin_set() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    let installs = dir.path().join("installs");
    let bin = installs.join("jq/1.7.1");
    fs::create_dir_all(&bin).unwrap();
    write_exec(&bin.join("jq"), "echo PINNED");
    write_cached_env(
        home.path(),
        "aqua:jqlang/jq",
        "aqua-jqlang-jq",
        "1.7.1",
        "{}",
    );
    fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let set_stdout = String::from_utf8_lossy(
        &common::lade(home.path())
            .current_dir(dir.path())
            .env("MISE_INSTALLS_DIR", &installs)
            .env("PATH", "/usr/bin")
            .args(["set", "jq", "."])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .into_owned();
    let restore = export_value(&set_stdout, "LADE_RESTORE").expect("LADE_RESTORE");
    let unset_stdout = String::from_utf8_lossy(
        &common::lade(home.path())
            .current_dir(dir.path())
            .env("MISE_INSTALLS_DIR", &installs)
            .env("LADE_RESTORE", restore)
            .args(["unset", "jq", "."])
            .assert()
            .success()
            .get_output()
            .stdout
            .clone(),
    )
    .into_owned();
    assert!(
        unset_stdout.contains("export PATH='/usr/bin'"),
        "{unset_stdout}"
    );
}

#[cfg(unix)]
#[test]
fn set_refuses_mise_toml_version_conflict() {
    let dir = tempdir().unwrap();
    let home = tempdir().unwrap();
    fs::write(
        dir.path().join("lade.yml"),
        "^jq:\n  jq: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    fs::write(dir.path().join("mise.toml"), "[tools]\njq = \"1.6.0\"\n").unwrap();
    common::lade(home.path())
        .current_dir(dir.path())
        .args(["set", "jq"])
        .assert()
        .failure()
        .code(1)
        .stderr(predicates::str::contains("different versions"));
}
