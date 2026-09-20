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
fn write_tools_includes_checksum() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("lade.lock");
    write_tools(
        &path,
        &[LockSlot {
            name: "jq".to_string(),
            version: "1.7.1".to_string(),
            backend: Some("aqua:jqlang/jq".to_string()),
            checksum: Some("sha256:abc".to_string()),
        }],
    )
    .unwrap();
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.contains("checksum = \"sha256:abc\""), "{body}");
    let slot = slot_for(&path, &["jq"]).unwrap();
    assert_eq!(slot.checksum.as_deref(), Some("sha256:abc"));
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

#[test]
fn write_tools_merges_into_existing_mise_lock() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("mise.lock");
    write_tools(
        &path,
        &[LockSlot {
            name: "node".to_string(),
            version: "24.16.0".to_string(),
            backend: None,
            checksum: None,
        }],
    )
    .unwrap();
    write_tools(
        &path,
        &[LockSlot {
            name: "jq".to_string(),
            version: "1.7.1".to_string(),
            backend: Some("aqua:jqlang/jq".to_string()),
            checksum: None,
        }],
    )
    .unwrap();
    let slots = read_tools(&path).unwrap();
    assert!(slots.iter().any(|slot| slot.name == "node"));
    assert!(
        slots
            .iter()
            .any(|slot| slot.name == "jq" && slot.version == "1.7.1")
    );
}
