use std::collections::HashMap;
use std::path::PathBuf;

use rustc_hash::FxHashSet;

fn should_mask(key: &str, sources: &HashMap<String, String>, maskable: &FxHashSet<String>) -> bool {
    sources
        .get(key)
        .is_some_and(|source| maskable.contains(source))
}

/// Resolved values whose config source was loaded by a masking provider.
pub fn secrets_for_redaction(
    env: &HashMap<String, String>,
    files: &HashMap<PathBuf, HashMap<String, String>>,
    sources: &HashMap<String, String>,
    maskable: &FxHashSet<String>,
) -> HashMap<String, String> {
    let mut redact = HashMap::new();
    for (key, value) in env
        .iter()
        .chain(files.values().flat_map(|vars| vars.iter()))
    {
        if should_mask(key, sources, maskable) {
            redact.insert(key.clone(), value.clone());
        }
    }
    redact
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustc_hash::FxHashSet;

    #[test]
    fn test_only_maskable_sources_included() {
        let env = HashMap::from([
            ("RAW".to_string(), "3".to_string()),
            ("SECRET".to_string(), "hunter2".to_string()),
        ]);
        let sources = HashMap::from([
            ("RAW".to_string(), "3".to_string()),
            ("SECRET".to_string(), "op://vault/item/field".to_string()),
        ]);
        let mut maskable = FxHashSet::default();
        maskable.insert("op://vault/item/field".to_string());
        let redact = secrets_for_redaction(&env, &HashMap::new(), &sources, &maskable);
        assert_eq!(redact.len(), 1);
        assert_eq!(redact.get("SECRET").unwrap(), "hunter2");
    }
}
