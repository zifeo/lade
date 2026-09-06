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
fn test_resolve_one_no_double_expansion() {
    // A value that itself looks like a variable reference must not be re-expanded.
    let vars = HashMap::from([
        ("A".into(), "$B".into()),
        ("B".into(), "should_not_appear".into()),
    ]);
    assert_eq!(resolve_one("$A", &vars).unwrap(), "$B");
}

#[test]
fn test_resolve_one_unmatched_open_brace_is_literal() {
    // "${FOO" has no closing brace — must not be treated as a variable reference.
    let vars = HashMap::from([("FOO".to_string(), "bar".to_string())]);
    assert_eq!(resolve_one("${FOO", &vars).unwrap(), "${FOO");
}

#[test]
fn test_resolve_one_trailing_brace_not_consumed() {
    // "$FOO}" — only "$FOO" is a variable reference; the "}" is literal.
    let vars = HashMap::from([("FOO".to_string(), "bar".to_string())]);
    assert_eq!(resolve_one("$FOO}", &vars).unwrap(), "bar}");
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

#[test]
fn template_tracks_braced_and_bare_references() {
    let template = Template::parse("sh://echo $HOME ${TOKEN}");
    assert_eq!(
        template.dependencies().collect::<Vec<_>>(),
        vec!["HOME", "TOKEN"]
    );
    assert_eq!(
        template
            .render(&HashMap::from([("TOKEN".into(), "value".into())]))
            .unwrap(),
        "sh://echo $HOME value"
    );
}

#[test]
fn dag_allows_unknown_bare_shell_variables() {
    Dag::new(HashMap::from([(
        "AUTHORIZATION".into(),
        Template::parse("sh://echo $HOME"),
    )]))
    .unwrap();
}

#[test]
fn template_normalizes_private_reference_for_shell() {
    let template = Template::parse("sh://echo ${.TOKEN}");
    assert_eq!(template.dependencies().collect::<Vec<_>>(), vec!["TOKEN"]);
    assert_eq!(template.shell_source(), "sh://echo ${TOKEN}");
}

#[test]
fn dag_rejects_missing_dependency() {
    let err = Dag::new(HashMap::from([(
        "HEADER".into(),
        Template::parse("Bearer ${TOKEN}"),
    )]))
    .unwrap_err();
    assert!(err.to_string().contains("missing dependency 'TOKEN'"));
}

#[test]
fn dag_rejects_cycles() {
    let err = Dag::new(HashMap::from([
        ("A".into(), Template::parse("${B}")),
        ("B".into(), Template::parse("${A}")),
    ]))
    .unwrap_err();
    assert!(err.to_string().contains("cyclic binding dependencies"));
}
