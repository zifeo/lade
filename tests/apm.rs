use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

#[test]
fn apm_yml_version_matches_crate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let apm = fs::read_to_string(root.join("apm.yml")).unwrap();
    assert!(apm.contains("name: lade"));
    assert!(
        apm.contains(&format!("version: {}", env!("CARGO_PKG_VERSION"))),
        "{apm}"
    );
}

#[test]
fn apm_skill_is_a_link_to_the_authoring_skill() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let agents = root.join(".agents/skills/lade/SKILL.md");
    let apm = root.join(".apm/skills/lade/SKILL.md");
    assert!(fs::symlink_metadata(&apm).unwrap().file_type().is_symlink());
    assert_eq!(
        fs::read_to_string(&agents).unwrap(),
        fs::read_to_string(&apm).unwrap()
    );
}

#[test]
fn apm_installs_this_package_into_a_consumer() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let src = fs::read_to_string(root.join(".agents/skills/lade/SKILL.md")).unwrap();
    if Command::new("apm").arg("--version").output().is_err() {
        return;
    }

    let pkg = tempdir().unwrap();
    let consumer = tempdir().unwrap();
    fs::create_dir_all(pkg.path().join(".agents/skills/lade")).unwrap();
    fs::create_dir_all(pkg.path().join(".apm/skills/lade")).unwrap();
    fs::copy(root.join("apm.yml"), pkg.path().join("apm.yml")).unwrap();
    fs::write(pkg.path().join(".agents/skills/lade/SKILL.md"), &src).unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        "../../../.agents/skills/lade/SKILL.md",
        pkg.path().join(".apm/skills/lade/SKILL.md"),
    )
    .unwrap();

    fs::write(
        consumer.path().join("apm.yml"),
        "name: lade-apm-smoke\nversion: 0.0.1\n",
    )
    .unwrap();
    let output = Command::new("apm")
        .current_dir(consumer.path())
        .env("CI", "1")
        .args([
            "install",
            pkg.path().to_str().unwrap(),
            "--target",
            "cursor",
        ])
        .output()
        .expect("apm install");
    assert!(
        output.status.success(),
        "apm install failed:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let deployed = consumer.path().join(".agents/skills/lade/SKILL.md");
    assert_eq!(fs::read_to_string(&deployed).unwrap(), src);
}
