use std::collections::HashMap;
use std::path::PathBuf;

use rustc_hash::FxHashSet;

fn should_mask(key: &str, sources: &HashMap<String, String>, maskable: &FxHashSet<String>) -> bool {
    // § qq076qjq2.a@r022e
    maskable.contains(key)
        || sources
            .get(key)
            .is_some_and(|source| maskable.contains(source))
}

/// Resolved values whose config source was loaded by a masking provider.
/// `private` holds the private bindings' values by name: the command never
/// gets them as such, but may still print one.
pub fn secrets_for_redaction(
    env: &HashMap<String, String>,
    files: &HashMap<PathBuf, HashMap<String, String>>,
    private: &HashMap<String, String>,
    sources: &HashMap<String, String>,
    maskable: &FxHashSet<String>,
) -> HashMap<String, String> {
    // § qq076qjq2.b@my9wm
    let mut redact = HashMap::new();
    for (key, value) in env
        .iter()
        .chain(files.values().flat_map(|vars| vars.iter()))
        .chain(private.iter())
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
        let redact =
            secrets_for_redaction(&env, &HashMap::new(), &HashMap::new(), &sources, &maskable);
        assert_eq!(redact.len(), 1);
        assert_eq!(redact.get("SECRET").unwrap(), "hunter2");
    }

    #[test]
    fn private_maskable_value_is_masked() {
        let private = HashMap::from([("TOKEN".to_string(), "hunter2".to_string())]);
        let mut maskable = FxHashSet::default();
        maskable.insert("TOKEN".to_string());
        let redact = secrets_for_redaction(
            &HashMap::new(),
            &HashMap::new(),
            &private,
            &HashMap::new(),
            &maskable,
        );
        assert_eq!(redact.get("TOKEN").map(String::as_str), Some("hunter2"));
    }
}
