use super::RuleReport;
use super::format::{elapsed_ms, format_rule_line, short_error, truncate_chars};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn elapsed_ms_rounds_to_micros() {
    let started = Instant::now() - Duration::from_micros(1234);
    let ms = elapsed_ms(started);
    assert!(ms >= 1.234);
    assert!(ms < 50.0);
}

#[test]
fn short_error_prefers_message_line() {
    let err = "Infisical error: EOF\nMessage: Project with ID 'abc' not found\nRequest: GET x";
    assert_eq!(short_error(err), "Project with ID 'abc' not found");
}

#[test]
fn short_error_prefers_connection_refused() {
    let err = "Vault error: EOF\nGet \"https://127.0.0.1:8200\": connection refused";
    assert!(short_error(err).contains("connection refused"));
    assert!(!short_error(err).contains('\n'));
}

#[test]
fn truncate_chars_adds_ellipsis() {
    assert_eq!(truncate_chars("abcd", 4), "abcd");
    assert_eq!(truncate_chars("abcdefghij", 7), "abcd...");
}

#[test]
fn error_sits_on_the_next_line() {
    let line = format_rule_line(&RuleReport {
        file: PathBuf::from("/tmp/lade.yml"),
        pattern: "^echo g".into(),
        when: "always",
        hydrate_ms: 115.335,
        providers: vec!["passbolt".into()],
        error: Some("logging in: connection refused".into()),
    });
    let lines: Vec<&str> = line.lines().collect();
    assert_eq!(lines.len(), 2);
    assert!(lines[0].contains("passbolt"));
    assert!(!lines[0].contains("error"));
    assert_eq!(lines[1], "    error  logging in: connection refused");
}
