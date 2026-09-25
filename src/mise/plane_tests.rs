use super::*;
use tempfile::tempdir;

fn with_home<T>(home: &std::path::Path, f: impl FnOnce() -> T) -> T {
    temp_env::with_var("HOME", Some(home.as_os_str()), f)
}

fn git(dir: &std::path::Path) {
    std::fs::create_dir_all(dir.join(".git")).unwrap();
}

fn write(path: &std::path::Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, body).unwrap();
}

#[test]
fn a1_git_and_yaml_at_root_is_lade() {
    let home = tempdir().unwrap();
    let proj = home.path().join("proj");
    git(&proj);
    write(&proj.join("lade.yaml"), ".:\n  X: raw://x\n");
    with_home(home.path(), || {
        let snap = scan(&proj);
        assert_eq!(
            snap.plane,
            Plane::Lade {
                lock: proj.join("lade.lock")
            }
        );
        assert_eq!(snap.project_git_root.as_deref(), Some(proj.as_path()));
    });
}

#[test]
fn a2_yaml_in_child_lock_at_git_root() {
    let home = tempdir().unwrap();
    let proj = home.path().join("proj");
    let app = proj.join("app");
    git(&proj);
    write(&app.join("lade.yaml"), ".:\n  X: raw://x\n");
    with_home(home.path(), || {
        let snap = scan(&app);
        assert_eq!(
            snap.plane,
            Plane::Lade {
                lock: proj.join("lade.lock")
            }
        );
        assert!(!app.join("lade.lock").exists());
    });
}

#[test]
fn a4_home_git_is_not_project_root() {
    let home = tempdir().unwrap();
    git(home.path());
    let proj = home.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    write(&proj.join("lade.yaml"), ".:\n  X: raw://x\n");
    with_home(home.path(), || {
        let snap = scan(&proj);
        assert_eq!(snap.plane, Plane::None);
        assert_eq!(snap.project_git_root, None);
    });
}

#[test]
fn a5_no_git_is_none() {
    let home = tempdir().unwrap();
    let proj = home.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    write(&proj.join("lade.yaml"), ".:\n  X: raw://x\n");
    with_home(home.path(), || {
        let snap = scan(&proj);
        assert_eq!(snap.plane, Plane::None);
    });
}

#[test]
fn a6_parent_mise_lock_is_the_plane() {
    let home = tempdir().unwrap();
    let proj = home.path().join("proj");
    let app = proj.join("app");
    git(&proj);
    write(&proj.join("mise.lock"), "lockfile_version = 1\n");
    write(&app.join("lade.yaml"), ".:\n  X: raw://x\n");
    with_home(home.path(), || {
        let snap = scan(&app);
        match snap.plane {
            Plane::Mise { dir, lock, .. } => {
                assert_eq!(dir, proj);
                assert_eq!(lock, proj.join("mise.lock"));
            }
            other => panic!("{other:?}"),
        }
        assert!(snap.leftover_lade_locks.is_empty());
    });
}

#[test]
fn a7_toml_only_creates_lock_write_path() {
    let home = tempdir().unwrap();
    let proj = home.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    write(&proj.join("mise.toml"), "[tools]\nnode = \"24\"\n");
    write(&proj.join("lade.yaml"), ".:\n  X: raw://x\n");
    with_home(home.path(), || {
        let snap = scan(&proj);
        match snap.plane {
            Plane::Mise {
                lock,
                toml_write,
                toml,
                ..
            } => {
                assert_eq!(lock, proj.join("mise.lock"));
                assert_eq!(toml_write, proj.join("mise.toml"));
                assert_eq!(toml.as_deref(), Some(proj.join("mise.toml").as_path()));
            }
            other => panic!("{other:?}"),
        }
    });
}

#[test]
fn a8_alternate_committed_toml_names() {
    for name in [
        ".mise.toml",
        "mise/config.toml",
        ".mise/config.toml",
        ".config/mise.toml",
    ] {
        let home = tempdir().unwrap();
        let proj = home.path().join("proj");
        write(&proj.join(name), "[tools]\nnode = \"24\"\n");
        with_home(home.path(), || {
            let snap = scan(&proj);
            assert!(snap.is_mise(), "{name}");
        });
    }
}

#[test]
fn a9_local_toml_is_not_a_plane() {
    let home = tempdir().unwrap();
    let proj = home.path().join("proj");
    git(&proj);
    write(&proj.join("mise.local.toml"), "[tools]\njq = \"1.6.0\"\n");
    write(&proj.join("lade.yaml"), ".:\n  X: raw://x\n");
    with_home(home.path(), || {
        let snap = scan(&proj);
        assert_eq!(
            snap.plane,
            Plane::Lade {
                lock: proj.join("lade.lock")
            }
        );
    });
}

#[test]
fn a10_home_mise_ignored_for_project() {
    let home = tempdir().unwrap();
    write(&home.path().join("mise.lock"), "lockfile_version = 1\n");
    write(&home.path().join("mise.toml"), "[tools]\njava = \"21\"\n");
    let proj = home.path().join("proj");
    git(&proj);
    write(&proj.join("lade.yaml"), ".:\n  X: raw://x\n");
    with_home(home.path(), || {
        let snap = scan(&proj);
        assert_eq!(
            snap.plane,
            Plane::Lade {
                lock: proj.join("lade.lock")
            }
        );
        assert!(!snap.is_mise());
    });
}

#[test]
fn a11_start_at_home_uses_home_mise() {
    let home = tempdir().unwrap();
    write(&home.path().join("mise.toml"), "[tools]\njava = \"21\"\n");
    write(&home.path().join("lade.yaml"), ".:\n  X: raw://x\n");
    with_home(home.path(), || {
        let snap = scan(home.path());
        assert!(snap.is_mise());
        match snap.plane {
            Plane::Mise { dir, .. } => assert_eq!(dir, home.path()),
            other => panic!("{other:?}"),
        }
    });
}

#[test]
fn a12_home_git_never_anchors_lade_lock() {
    let home = tempdir().unwrap();
    git(home.path());
    write(&home.path().join("lade.yaml"), ".:\n  X: raw://x\n");
    with_home(home.path(), || {
        let snap = scan(home.path());
        assert_eq!(snap.plane, Plane::None);
        assert_eq!(snap.project_git_root, None);
    });
}

#[test]
fn a14_split_lock_child_toml_parent() {
    let home = tempdir().unwrap();
    let proj = home.path().join("proj");
    let app = proj.join("app");
    write(&proj.join("mise.toml"), "[tools]\nnode = \"24\"\n");
    write(&app.join("mise.lock"), "lockfile_version = 1\n");
    with_home(home.path(), || {
        let snap = scan(&app);
        assert!(snap.split_warning);
        match snap.plane {
            Plane::Mise {
                dir,
                other_toml,
                other_lock,
                toml_write,
                ..
            } => {
                assert_eq!(dir, app);
                assert_eq!(
                    other_toml.as_deref(),
                    Some(proj.join("mise.toml").as_path())
                );
                assert_eq!(other_lock, None);
                assert_eq!(toml_write, app.join("mise.toml"));
            }
            other => panic!("{other:?}"),
        }
    });
}

#[test]
fn a15_mise_wins_over_sibling_lade_lock() {
    let home = tempdir().unwrap();
    let proj = home.path().join("proj");
    write(&proj.join("mise.lock"), "lockfile_version = 1\n");
    write(&proj.join("lade.lock"), "lockfile_version = 1\n");
    with_home(home.path(), || {
        let snap = scan(&proj);
        assert!(snap.is_mise());
        assert_eq!(snap.leftover_lade_locks, vec![proj.join("lade.lock")]);
        assert_eq!(snap.lock_path(), Some(proj.join("mise.lock").as_path()));
    });
}

#[test]
fn a18_gitfile_is_worktree_root() {
    let home = tempdir().unwrap();
    let proj = home.path().join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    write(&proj.join(".git"), "gitdir: /tmp/worktrees/proj\n");
    write(&proj.join("lade.yaml"), ".:\n  X: raw://x\n");
    with_home(home.path(), || {
        let snap = scan(&proj);
        assert_eq!(snap.project_git_root.as_deref(), Some(proj.as_path()));
        assert_eq!(
            snap.plane,
            Plane::Lade {
                lock: proj.join("lade.lock")
            }
        );
    });
}

#[test]
fn a16_leftover_child_lade_lock() {
    let home = tempdir().unwrap();
    let proj = home.path().join("proj");
    let app = proj.join("app");
    write(&proj.join("mise.lock"), "lockfile_version = 1\n");
    write(&app.join("lade.lock"), "lockfile_version = 1\n");
    with_home(home.path(), || {
        let snap = scan(&app);
        assert!(snap.is_mise());
        assert!(snap.leftover_lade_locks.contains(&app.join("lade.lock")));
    });
}

#[test]
fn d2_one_pass_to_home() {
    let home = tempdir().unwrap();
    let start = home.path().join("a/b/c");
    std::fs::create_dir_all(&start).unwrap();
    write(&start.join("lade.yaml"), ".:\n  X: raw://c\n");
    write(&home.path().join("a/b/lade.yaml"), ".:\n  X: raw://b\n");
    write(&home.path().join("a/lade.yaml"), ".:\n  X: raw://a\n");
    write(&home.path().join("lade.yaml"), ".:\n  X: raw://home\n");
    with_home(home.path(), || {
        let snap = scan(&start);
        let files = snap.yaml_files;
        assert_eq!(
            files,
            vec![
                start.join("lade.yaml"),
                home.path().join("a/b/lade.yaml"),
                home.path().join("a/lade.yaml"),
                home.path().join("lade.yaml"),
            ]
        );
    });
}
