use crate::mise::lock::*;
use tempfile::tempdir;

#[test]
fn reads_array_slot() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("mise.lock"),
        r#"
lockfile_version = 1

[[tools.jq]]
version = "1.7.1"
backend = "aqua:jqlang/jq"
"#,
    )
    .unwrap();
    let slot = slot_for(&dir.path().join("mise.lock"), &["jq"]).unwrap();
    assert_eq!(slot.version, "1.7.1");
    assert_eq!(slot.backend.as_deref(), Some("aqua:jqlang/jq"));
    assert!(agrees(&slot, "1.7.1", "aqua:jqlang/jq"));
    assert!(!agrees(&slot, "1.8.0", "aqua:jqlang/jq"));
    assert!(!agrees(&slot, "1.7.1", "github:jqlang/jq"));
}

#[test]
fn reads_mise_lock_v2_with_platforms() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.lock"),
        r#"
# @generated
lockfile_version = 2

[[tools."aqua:jqlang/jq"]]
version = "1.7.1"
backend = "aqua:jqlang/jq"
specifiers = ["1.7.1"]

[tools."aqua:jqlang/jq"."platforms.macos-arm64"]
checksum = "sha256:abc"
url = "https://example.com/jq"
"#,
    )
    .unwrap();
    let slot = slot_for(&dir.path().join("lade.lock"), &["jq", "aqua:jqlang/jq"]).unwrap();
    assert_eq!(slot.version, "1.7.1");
    assert_eq!(slot.backend.as_deref(), Some("aqua:jqlang/jq"));
    assert!(is_generated(&dir.path().join("lade.lock")));
}

#[test]
fn walk_finds_parent_lock() {
    let root = tempdir().unwrap();
    let child = root.path().join("child");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(
        root.path().join("mise.lock"),
        "[[tools.jq]]\nversion = \"1.7.1\"\n",
    )
    .unwrap();
    let found = find_lock(&child).unwrap();
    assert_eq!(found, root.path().join("mise.lock"));
}

#[test]
fn walk_stops_at_home() {
    let root = tempdir().unwrap();
    let home = root.path().join("home");
    let proj = home.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(
        root.path().join("mise.lock"),
        "[[tools.jq]]\nversion = \"1\"\n",
    )
    .unwrap();
    std::fs::write(home.join("mise.lock"), "[[tools.jq]]\nversion = \"2\"\n").unwrap();
    temp_env::with_var("HOME", Some(home.as_os_str()), || {
        let found = find_lock(&proj).unwrap();
        assert_eq!(found, home.join("mise.lock"));
    });
}

#[test]
fn missing_name_is_none() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("mise.lock"),
        "[[tools.jq]]\nversion = \"1.7.1\"\n",
    )
    .unwrap();
    assert!(slot_for(&dir.path().join("mise.lock"), &["node"]).is_none());
}

#[test]
fn slot_for_backend_id_key() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("mise.lock"),
        "[[tools.\"aqua:jqlang/jq\"]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n",
    )
    .unwrap();
    let slot = slot_for(
        &dir.path().join("mise.lock"),
        &["jq", "aqua:jqlang/jq", "aqua-jqlang-jq"],
    )
    .unwrap();
    assert_eq!(slot.name, "aqua:jqlang/jq");
    assert_eq!(slot.version, "1.7.1");
    assert!(agrees(&slot, "1.7.1", "aqua:jqlang/jq"));
}

#[test]
fn agrees_when_lock_omits_backend() {
    let slot = LockSlot {
        name: "jq".to_string(),
        version: "1.7.1".to_string(),
        backend: None,
        checksum: None,
    };
    assert!(agrees(&slot, "1.7.1", "aqua:jqlang/jq"));
    assert!(!agrees(&slot, "1.6.0", "aqua:jqlang/jq"));
}

#[test]
fn agrees_when_lock_satisfies_range() {
    let slot = LockSlot {
        name: "op".to_string(),
        version: "2.31.0".to_string(),
        backend: Some("aqua:1password/cli".to_string()),
        checksum: None,
    };
    assert!(agrees(&slot, ">=2.18.0", "aqua:1password/cli"));
    assert!(!agrees(&slot, ">=3.0.0", "aqua:1password/cli"));
    assert!(!agrees(&slot, "2.31.0", "aqua:other/op"));
    assert!(agrees(&slot, "latest", "aqua:1password/cli"));
}

#[test]
fn generated_lock_detects_mise_output() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("lade.lock");
    std::fs::write(
        &path,
        "[[tools.fish]]\nversion = \"4.9.3\"\nbackend = \"aqua:fish-shell/fish-shell\"\n",
    )
    .unwrap();
    assert!(!is_generated(&path));
    std::fs::write(
        &path,
        "# @generated\nlockfile_version = 2\n\n[[tools.fish]]\nversion = \"4.9.3\"\nbackend = \"aqua:fish-shell/fish-shell\"\n\n[tools.fish.\"platforms.macos-arm64\"]\nurl = \"https://example.com/fish.pkg\"\n",
    )
    .unwrap();
    assert!(is_generated(&path));
}

#[test]
fn no_lock_is_none() {
    let dir = tempdir().unwrap();
    assert!(find_lock(dir.path()).is_none());
}

#[test]
fn child_lock_walk_finds_parent_tool() {
    let root = tempdir().unwrap();
    let child = root.path().join("apps/web");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(
        root.path().join("lade.lock"),
        "[[tools.jq]]\nversion = \"1.7.1\"\nbackend = \"aqua:jqlang/jq\"\n",
    )
    .unwrap();
    std::fs::write(
        child.join("lade.lock"),
        "[[tools.node]]\nversion = \"24.16.0\"\n",
    )
    .unwrap();
    let found = find_lock(&child).unwrap();
    assert_eq!(found, child.join("lade.lock"));
    assert!(slot_for(&found, &["jq", "aqua:jqlang/jq"]).is_none());
    assert!(slot_for_walk(&child, &["jq", "aqua:jqlang/jq"]).is_some());
    assert!(slot_for_walk(&child, &["node"]).is_some());
}

#[test]
fn path_in_keeps_native_mise_lock() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("mise.lock"), "lockfile_version = 1\n").unwrap();
    assert_eq!(path_in(dir.path()), dir.path().join("mise.lock"));
    let empty = tempdir().unwrap();
    assert_eq!(path_in(empty.path()), empty.path().join("lade.lock"));
}
