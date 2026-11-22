use std::path::Path;

pub fn git_stamp(cwd: &Path) -> (Option<String>, Option<String>) {
    // § ck86fwnt7.a@v1yas
    let mut dir = cwd.to_path_buf();
    loop {
        let git = dir.join(".git");
        if git.is_dir() {
            return (Some(repo_key(&dir)), read_head(&git));
        }
        if git.is_file() {
            return (Some(repo_key(&dir)), read_worktree_head(&git));
        }
        if !dir.pop() {
            return (None, None);
        }
    }
}

fn repo_key(dir: &Path) -> String {
    dir.canonicalize()
        .unwrap_or_else(|_| dir.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn read_worktree_head(gitfile: &Path) -> Option<String> {
    let text = std::fs::read_to_string(gitfile).ok()?;
    let gitdir = text
        .lines()
        .find_map(|line| line.strip_prefix("gitdir:"))?
        .trim();
    let gitdir = {
        let raw = Path::new(gitdir);
        if raw.is_absolute() {
            raw.to_path_buf()
        } else {
            gitfile.parent()?.join(raw)
        }
    };
    read_head(&gitdir)
}

fn read_head(git: &Path) -> Option<String> {
    let head = std::fs::read_to_string(git.join("HEAD")).ok()?;
    let head = head.trim();
    match head.strip_prefix("ref: ") {
        Some(name) => read_ref(git, name.trim()),
        None => hex40(head),
    }
}

/// A linked worktree keeps its branches in the common dir, and `git gc`
/// moves loose refs into `packed-refs`.
fn read_ref(git: &Path, name: &str) -> Option<String> {
    let common = match std::fs::read_to_string(git.join("commondir")) {
        Ok(rel) => git.join(rel.trim()),
        Err(_) => git.to_path_buf(),
    };
    for dir in [git, common.as_path()] {
        if let Ok(val) = std::fs::read_to_string(dir.join(name)) {
            return hex40(val.trim());
        }
    }
    let packed = std::fs::read_to_string(common.join("packed-refs")).ok()?;
    packed.lines().find_map(|line| {
        let (hash, rest) = line.split_once(' ')?;
        (rest.trim() == name).then(|| hex40(hash)).flatten()
    })
}

fn hex40(s: &str) -> Option<String> {
    (s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit())).then(|| s.to_string())
}
