use std::path::{Path, PathBuf};

use crate::config::at_user_home;

pub fn walk_up<T, F>(start: &Path, mut find: F) -> Option<T>
where
    F: FnMut(&Path) -> Option<T>,
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
