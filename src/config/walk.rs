use std::path::Path;

use sha2::{Digest, Sha256};

/// SHA-256 of every yaml on the CWD→HOME walk. Comments change the hash.
pub fn hash_yaml_files(files: &[impl AsRef<Path>]) -> [u8; 32] {
    // § je3g2tk0d.b@7q6mz
    let read = files
        .iter()
        .map(|file| {
            let path = file.as_ref();
            (
                path.to_string_lossy().into_owned(),
                std::fs::read(path).unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();
    hash_walk(&read)
}

/// The same hash over paths and contents already in memory: a laptop pins a
/// remote walk from the bytes it showed.
pub fn hash_walk(files: &[(String, Vec<u8>)]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for (path, content) in files {
        hasher.update((path.len() as u64).to_be_bytes());
        hasher.update(path.as_bytes());
        hasher.update((content.len() as u64).to_be_bytes());
        hasher.update(content);
    }
    hasher.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn comment_changes_the_hash() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        std::fs::write(&path, "\"cmd\":\n  KEY: op://v/i/f\n").unwrap();
        let first = hash_yaml_files(&[&path]);
        std::fs::write(&path, "\"cmd\":\n  # hi\n  KEY: op://v/i/f\n").unwrap();
        let second = hash_yaml_files(&[&path]);
        assert_ne!(first, second);
    }

    #[test]
    fn walk_bytes_hash_as_the_files() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        std::fs::write(&path, "\"cmd\":\n  KEY: op://v/i/f\n").unwrap();
        let bytes = vec![(
            path.to_string_lossy().into_owned(),
            std::fs::read(&path).unwrap(),
        )];
        assert_eq!(hash_yaml_files(&[&path]), hash_walk(&bytes));
    }
}
