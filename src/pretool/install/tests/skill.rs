use super::super::agent::Agent;
use super::super::skill::{SKILL_MD, is_lade_skill, skill_is_current, write_skill};
use super::super::write::refresh_at;
use super::stale_official_skill;

#[test]
fn skill_path_sits_under_the_agent_home() {
    let home = std::path::Path::new("/tmp/home");
    assert_eq!(
        Agent::Cursor.skill_path(home),
        home.join(".cursor")
            .join("skills")
            .join("lade")
            .join("SKILL.md")
    );
    assert_eq!(
        Agent::OpenCode.skill_path(home),
        home.join(".config")
            .join("opencode")
            .join("skills")
            .join("lade")
            .join("SKILL.md")
    );
}

#[test]
fn bundled_skill_is_lade_managed() {
    assert!(is_lade_skill(SKILL_MD));
    assert!(skill_is_current(SKILL_MD));
    assert!(is_lade_skill(stale_official_skill()));
    assert!(!skill_is_current(stale_official_skill()));
    assert!(!is_lade_skill("# mine\n"));
    assert!(!is_lade_skill("---\nname: lade\ndescription: mine\n---\n"));
}

#[test]
fn refresh_updates_stale_managed_skill_and_skips_unmanaged() {
    let home = tempfile::tempdir().unwrap();
    let path = Agent::Codex.skill_path(home.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, stale_official_skill()).unwrap();
    let foreign = Agent::Claude.skill_path(home.path());
    std::fs::create_dir_all(foreign.parent().unwrap()).unwrap();
    std::fs::write(&foreign, "# mine\n").unwrap();
    refresh_at(home.path(), home.path());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), SKILL_MD);
    assert_eq!(std::fs::read_to_string(&foreign).unwrap(), "# mine\n");
}

#[test]
fn write_skill_leaves_unmanaged_files() {
    let home = tempfile::tempdir().unwrap();
    let path = Agent::Claude.skill_path(home.path());
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "# mine\n").unwrap();
    let out = write_skill(Agent::Claude, &path).unwrap();
    assert_eq!(out.verb, super::super::paths::ItemVerb::Unmanaged);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "# mine\n");
}

#[test]
fn refresh_does_not_create_a_missing_skill() {
    let home = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(home.path().join(".codex")).unwrap();
    refresh_at(home.path(), home.path());
    assert!(!Agent::Codex.skill_path(home.path()).exists());
}
