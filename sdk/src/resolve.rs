use std::collections::HashMap;

use anyhow::Result;
use once_cell::sync::Lazy;
use regex::Regex;

static VAR: Lazy<Regex> = Lazy::new(|| Regex::new(r"(\$\{?(\w+)\}?)").unwrap());

pub fn resolve(
    kvs: &HashMap<String, String>,
    existing_vars: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    kvs.iter()
        .map(|(key, value)| resolve_one(value, existing_vars).map(|v| (key.clone(), v)))
        .collect()
}

pub fn resolve_one(value: &str, existing_vars: &HashMap<String, String>) -> Result<String> {
    Ok(VAR.captures_iter(value).fold(value.to_string(), |agg, c| {
        agg.replace(&c[1], existing_vars.get(&c[2]).unwrap_or(&"".to_string()))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_resolve_one_no_vars() {
        assert_eq!(
            resolve_one("hello world", &HashMap::new()).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn test_resolve_one_dollar_var() {
        let vars = HashMap::from([("FOO".to_string(), "bar".to_string())]);
        assert_eq!(resolve_one("prefix_$FOO", &vars).unwrap(), "prefix_bar");
    }

    #[test]
    fn test_resolve_one_braces_var() {
        let vars = HashMap::from([("FOO".to_string(), "bar".to_string())]);
        assert_eq!(
            resolve_one("prefix_${FOO}_suffix", &vars).unwrap(),
            "prefix_bar_suffix"
        );
    }

    #[test]
    fn test_resolve_one_multiple_vars() {
        let vars = HashMap::from([
            ("A".to_string(), "hello".to_string()),
            ("B".to_string(), "world".to_string()),
        ]);
        assert_eq!(resolve_one("$A $B", &vars).unwrap(), "hello world");
    }

    #[test]
    fn test_resolve_one_unknown_var_empty() {
        assert_eq!(
            resolve_one("val/$MISSING", &HashMap::new()).unwrap(),
            "val/"
        );
    }

    #[test]
    fn test_resolve_one_adjacent_braced_vars() {
        let vars = HashMap::from([
            ("A".to_string(), "foo".to_string()),
            ("B".to_string(), "bar".to_string()),
        ]);
        assert_eq!(resolve_one("${A}${B}", &vars).unwrap(), "foobar");
    }

    #[test]
    fn test_resolve_one_word_boundary_without_braces() {
        let vars = HashMap::from([("FOO".to_string(), "bar".to_string())]);
        assert_eq!(resolve_one("$FOO_SUFFIX", &vars).unwrap(), "");
    }

    #[test]
    fn test_resolve_batch() {
        let kvs = HashMap::from([
            ("URL".to_string(), "https://$HOST/api".to_string()),
            ("STATIC".to_string(), "literal".to_string()),
        ]);
        let vars = HashMap::from([("HOST".to_string(), "example.com".to_string())]);
        let result = resolve(&kvs, &vars).unwrap();
        assert_eq!(result.get("URL").unwrap(), "https://example.com/api");
        assert_eq!(result.get("STATIC").unwrap(), "literal");
    }
}
