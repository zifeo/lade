use std::path::Path;

use sha2::{Digest, Sha256};

/// SHA-256 of every yaml on the CWD→HOME walk. Comments change the hash.
pub fn hash_yaml_files(files: &[impl AsRef<Path>]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for file in files {
        let path = file.as_ref();
        let path_bytes = path.to_string_lossy();
        let path_bytes = path_bytes.as_bytes();
        let content = std::fs::read(path).unwrap_or_default();
        hasher.update((path_bytes.len() as u64).to_be_bytes());
        hasher.update(path_bytes);
        hasher.update((content.len() as u64).to_be_bytes());
        hasher.update(&content);
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
}
