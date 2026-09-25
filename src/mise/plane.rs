use std::path::{Path, PathBuf};

use crate::config::at_user_home;

const COMMITTED_TOML: &[&str] = &[
    "mise.toml",
    ".mise.toml",
    "mise/config.toml",
    ".mise/config.toml",
    ".config/mise.toml",
];

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub yaml_files: Vec<PathBuf>,
    pub project_git_root: Option<PathBuf>,
    pub plane: Plane,
    pub leftover_lade_locks: Vec<PathBuf>,
    /// Walk length. Read by plane tests. Production uses the plane, not the count.
    #[allow(dead_code)]
    pub dirs_visited: usize,
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
    let mut nearest_lock = None;
    let mut nearest_toml = None;

    loop {
        dirs_visited += 1;
        let at_home = at_user_home(&path);
        // HOME mise is not a write plane unless this scan started at HOME (~/lade.yaml).
        let counts = !at_home || start_is_home;

        push_if_file(&mut yaml_files, path.join("lade.yaml"));
        push_if_file(&mut yaml_files, path.join("lade.yml"));

        let git = path.join(".git");
        // People git-init HOME; that is never project_git_root.
        if project_git_root.is_none() && !at_home && (git.is_dir() || git.is_file()) {
            project_git_root = Some(path.clone());
        }

        push_if_file(&mut lade_locks, path.join("lade.lock"));

        if counts {
            let mise_lock = path.join("mise.lock");
            if nearest_lock.is_none() && mise_lock.is_file() {
                nearest_lock = Some(Hit {
                    order: dirs_visited,
                    dir: path.clone(),
                    path: mise_lock,
                });
            }
            if nearest_toml.is_none()
                && let Some(toml) = committed_toml_in(&path)
            {
                nearest_toml = Some(Hit {
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

    let (plane, split_warning) = decide(nearest_lock, nearest_toml, project_git_root.as_deref());
    let leftover_lade_locks = leftover_of(lade_locks, &plane);

    Snapshot {
        yaml_files,
        project_git_root,
        plane,
        leftover_lade_locks,
        dirs_visited,
        split_warning,
    }
}

fn leftover_of(lade_locks: Vec<PathBuf>, plane: &Plane) -> Vec<PathBuf> {
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
