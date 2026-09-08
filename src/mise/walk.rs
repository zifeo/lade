use std::path::{Path, PathBuf};

pub fn at_user_home(path: &Path) -> bool {
    directories::UserDirs::new().is_some_and(|user| user.home_dir() == path)
}

pub fn walk_up<F>(start: &Path, mut find: F) -> Option<PathBuf>
where
    F: FnMut(&Path) -> Option<PathBuf>,
{
    let mut path = start.to_path_buf();
    loop {
        if let Some(hit) = find(&path) {
            return Some(hit);
        }
        if at_user_home(&path) {
            return None;
        }
        path = path.parent()?.to_path_buf();
    }
}

pub fn walk_up_all<F>(start: &Path, mut find: F) -> Vec<PathBuf>
where
    F: FnMut(&Path) -> Vec<PathBuf>,
{
    let mut path = start.to_path_buf();
    let mut out = Vec::new();
    loop {
        out.extend(find(&path));
        if at_user_home(&path) {
            break;
        }
        let Some(parent) = path.parent() else {
            break;
        };
        path = parent.to_path_buf();
    }
    out
}
