use std::path::{Path, PathBuf};

use crate::config::at_user_home;

const COMMITTED_TOML: &[&str] = &[
    "mise.toml",
    ".mise.toml",
    "mise/config.toml",
    ".mise/config.toml",
    ".config/mise.toml",
    ".config/mise/config.toml",
];

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub yaml_files: Vec<PathBuf>,
    pub project_git_root: Option<PathBuf>,
    pub plane: Plane,
    pub leftover_lade_locks: Vec<PathBuf>,
    pub split_warning: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Plane {
    Mise {
        dir: PathBuf,
        lock: PathBuf,
        /// Existing committed toml in the plane dir, if any.
        toml: Option<PathBuf>,
        /// Where to upsert [tools].
        toml_write: PathBuf,
        other_toml: Option<PathBuf>,
        other_lock: Option<PathBuf>,
    },
    Lade {
        lock: PathBuf,
    },
    None,
}

impl Snapshot {
    pub fn yaml_dirs(&self) -> Vec<PathBuf> {
        let mut out = Vec::new();
        for file in &self.yaml_files {
            let Some(dir) = file.parent() else {
                continue;
            };
            if !out.iter().any(|seen| seen == dir) {
                out.push(dir.to_path_buf());
            }
        }
        out
    }

    pub fn lock_path(&self) -> Option<&Path> {
        match &self.plane {
            Plane::Mise { lock, .. } => Some(lock.as_path()),
            Plane::Lade { lock } => Some(lock.as_path()),
            Plane::None => None,
        }
    }

    pub fn is_mise(&self) -> bool {
        matches!(self.plane, Plane::Mise { .. })
    }
}

struct Hit {
    order: usize,
    dir: PathBuf,
    path: PathBuf,
}

pub fn scan(start: &Path) -> Snapshot {
    let start_is_home = at_user_home(start);

    let mut path = start.to_path_buf();
    let mut yaml_files = Vec::new();
    let mut project_git_root = None;
    let mut lade_locks = Vec::new();
    let mut dirs_visited = 0;
    let mut locks = Vec::new();
    let mut tomls = Vec::new();

    loop {
        dirs_visited += 1;
        let at_home = at_user_home(&path);
        // HOME mise is not a write plane unless this scan started at HOME (~/lade.yaml).
        // § one a45aqmr7m.b@0ncjs
        let counts = !at_home || start_is_home;

        push_if_file(&mut yaml_files, path.join("lade.yaml"));
        push_if_file(&mut yaml_files, path.join("lade.yml"));

        let git = path.join(".git");
        // People git-init HOME; that is never project_git_root.
        if project_git_root.is_none() && !at_home && (git.is_dir() || git.is_file()) {
            // § a45aqmr7m.c@88mvk
            project_git_root = Some(path.clone());
        }

        push_if_file(&mut lade_locks, path.join("lade.lock"));
        // Above the git root a mise file is not the repo's, whatever git says of it.
        // § one a45aqmr7m.a@2egb5
        let in_repo = project_git_root.as_ref().is_none_or(|root| *root == path);

        if counts && in_repo {
            // § a45aqmr7m.a@fk6f4
            let mise_lock = path.join("mise.lock");
            if mise_lock.is_file() {
                locks.push(Hit {
                    order: dirs_visited,
                    dir: path.clone(),
                    path: mise_lock,
                });
            }
            if let Some(toml) = committed_toml_in(&path) {
                tomls.push(Hit {
                    order: dirs_visited,
                    dir: path.clone(),
                    path: toml,
                });
            }
        }

        if at_home {
            break;
        }
        let Some(parent) = path.parent() else {
            break;
        };
        path = parent.to_path_buf();
    }

    let (nearest_lock, nearest_toml) = if project_git_root.is_some() {
        nearest_committed(locks, tomls)
    } else {
        (locks.into_iter().next(), tomls.into_iter().next())
    };
    let (plane, split_warning) = decide(nearest_lock, nearest_toml, project_git_root.as_deref());
    let leftover_lade_locks = leftover_of(lade_locks, &plane);

    Snapshot {
        yaml_files,
        project_git_root,
        plane,
        leftover_lade_locks,
        split_warning,
    }
}

/// In a git repo, the nearest mise file git does not report as untracked
/// names the plane directory, and the other plane file in that directory
/// joins it, tracked or not, since setup writes it there.
fn nearest_committed(locks: Vec<Hit>, tomls: Vec<Hit>) -> (Option<Hit>, Option<Hit>) {
    // § a45aqmr7m.f@qn3cc
    let mut lock = None;
    let mut rest_locks = Vec::new();
    for hit in locks {
        if lock.is_none() && !git_reports_untracked(&hit) {
            lock = Some(hit);
        } else {
            rest_locks.push(hit);
        }
    }
    let mut toml = None;
    let mut rest_tomls = Vec::new();
    for hit in tomls {
        if toml.is_none() && !git_reports_untracked(&hit) {
            toml = Some(hit);
        } else {
            rest_tomls.push(hit);
        }
    }
    let nearest = match (&lock, &toml) {
        (Some(l), Some(t)) => Some(l.order.min(t.order)),
        (Some(l), None) => Some(l.order),
        (None, Some(t)) => Some(t.order),
        (None, None) => None,
    };
    let Some(order) = nearest else {
        return (None, None);
    };
    if toml.as_ref().is_none_or(|t| t.order > order)
        && let Some(index) = rest_tomls.iter().position(|t| t.order == order)
    {
        toml = Some(rest_tomls.swap_remove(index));
    }
    if lock.as_ref().is_none_or(|l| l.order > order)
        && let Some(index) = rest_locks.iter().position(|l| l.order == order)
    {
        lock = Some(rest_locks.swap_remove(index));
    }
    (lock, toml)
}

/// True only when git, run in the file's directory, lists it as not tracked.
/// Without a git binary or a readable repository there is no such report,
/// and the file counts as present, as it did before Lade asked git.
fn git_reports_untracked(hit: &Hit) -> bool {
    // A wrap scans once per pin and per diary binding: ask git once per file.
    static SEEN: std::sync::Mutex<Vec<(PathBuf, bool)>> = std::sync::Mutex::new(Vec::new());
    let mut seen = SEEN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((_, untracked)) = seen.iter().find(|(path, _)| *path == hit.path) {
        return *untracked;
    }
    let untracked = ask_git_untracked(hit);
    seen.push((hit.path.clone(), untracked));
    untracked
}

fn ask_git_untracked(hit: &Hit) -> bool {
    let Ok(name) = hit.path.strip_prefix(&hit.dir) else {
        return false;
    };
    std::process::Command::new("git")
        .arg("-C")
        .arg(&hit.dir)
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(name)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.code() == Some(1))
}

fn leftover_of(lade_locks: Vec<PathBuf>, plane: &Plane) -> Vec<PathBuf> {
    // § a45aqmr7m.d@rrb1s
    match plane {
        Plane::Lade { lock } => lade_locks.into_iter().filter(|path| path != lock).collect(),
        Plane::Mise { .. } | Plane::None => lade_locks,
    }
}

fn decide(
    nearest_lock: Option<Hit>,
    nearest_toml: Option<Hit>,
    project_git_root: Option<&Path>,
) -> (Plane, bool) {
    match (nearest_lock, nearest_toml) {
        (None, None) => {
            // § a45aqmr7m.c@y7mxc
            let plane = match project_git_root {
                Some(git) => Plane::Lade {
                    lock: git.join("lade.lock"),
                },
                None => Plane::None,
            };
            (plane, false)
        }
        (Some(lock), None) => {
            let toml_write = lock.dir.join("mise.toml");
            (
                Plane::Mise {
                    dir: lock.dir,
                    lock: lock.path,
                    toml: None,
                    toml_write,
                    other_toml: None,
                    other_lock: None,
                },
                false,
            )
        }
        (None, Some(toml)) => {
            let lock = toml.dir.join("mise.lock");
            (
                Plane::Mise {
                    dir: toml.dir,
                    lock,
                    toml: Some(toml.path.clone()),
                    toml_write: toml.path,
                    other_toml: None,
                    other_lock: None,
                },
                false,
            )
        }
        (Some(lock), Some(toml)) if lock.dir == toml.dir => (
            Plane::Mise {
                dir: lock.dir,
                lock: lock.path,
                toml: Some(toml.path.clone()),
                toml_write: toml.path,
                other_toml: None,
                other_lock: None,
            },
            false,
        ),
        (Some(lock), Some(toml)) if lock.order < toml.order => {
            // § a45aqmr7m.e@e13d3
            let toml_write = lock.dir.join("mise.toml");
            (
                Plane::Mise {
                    dir: lock.dir,
                    lock: lock.path,
                    toml: None,
                    toml_write,
                    other_toml: Some(toml.path),
                    other_lock: None,
                },
                true,
            )
        }
        (Some(lock), Some(toml)) => {
            // § a45aqmr7m.e@cwkkp
            let lock_write = toml.dir.join("mise.lock");
            (
                Plane::Mise {
                    dir: toml.dir,
                    lock: lock_write,
                    toml: Some(toml.path.clone()),
                    toml_write: toml.path,
                    other_toml: None,
                    other_lock: Some(lock.path),
                },
                true,
            )
        }
    }
}

fn committed_toml_in(dir: &Path) -> Option<PathBuf> {
    COMMITTED_TOML
        .iter()
        .map(|name| dir.join(name))
        .find(|path| path.is_file())
}

fn push_if_file(out: &mut Vec<PathBuf>, path: PathBuf) {
    if path.is_file() {
        out.push(path);
    }
}

#[cfg(test)]
#[path = "plane_tests.rs"]
mod tests;
