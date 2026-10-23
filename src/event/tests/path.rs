use super::*;

#[test]
fn db_path_override_and_project_dirs() {
    let dir = tempfile::tempdir().unwrap();
    let override_path = dir.path().join("events.db");
    temp_env::with_var(
        "LADE_EVENTS_PATH",
        Some(override_path.to_str().unwrap()),
        || {
            assert_eq!(db_path(), override_path);
        },
    );
    let home = tempfile::tempdir().unwrap();
    temp_env::with_vars(
        [
            ("LADE_EVENTS_PATH", None),
            ("HOME", Some(home.path().to_str().unwrap())),
        ],
        || {
            let expected = directories::ProjectDirs::from("com", "zifeo", "lade")
                .expect("cannot get directory for project")
                .data_local_dir()
                .join("events.db");
            assert_eq!(db_path(), expected);
        },
    );
}

#[test]
fn a_unit_test_on_the_real_home_never_reaches_its_diary() {
    let real = nix::unistd::User::from_uid(nix::unistd::getuid())
        .unwrap()
        .unwrap()
        .dir;
    temp_env::with_vars(
        [
            ("LADE_EVENTS_PATH", None),
            ("HOME", Some(real.to_str().unwrap())),
        ],
        || {
            assert_eq!(db_path(), crate::test_state::dir().join("events.db"));
        },
    );
}

#[test]
fn git_stamp_treats_gitfile_as_worktree_root() {
    let dir = tempfile::tempdir().unwrap();
    let worktree = dir.path().join("wt");
    let gitdir = dir.path().join("main.git");
    std::fs::create_dir(&worktree).unwrap();
    std::fs::create_dir(&gitdir).unwrap();
    std::fs::write(
        gitdir.join("HEAD"),
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n",
    )
    .unwrap();
    std::fs::write(
        worktree.join(".git"),
        format!("gitdir: {}\n", gitdir.display()),
    )
    .unwrap();
    let (repo, commit) = git_stamp(&worktree);
    assert_eq!(
        repo.as_deref(),
        Some(worktree.canonicalize().unwrap().to_str().unwrap())
    );
    assert_eq!(
        commit.as_deref(),
        Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
    );
}
