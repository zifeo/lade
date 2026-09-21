use crate::config::*;
use std::time::Instant;
use tempfile::tempdir;

#[test]
fn collect_two_hundred_rules_stays_under_a_second() {
    let dir = tempdir().unwrap();
    let mut body = String::from(".:\n  DEFAULT: val\n");
    for i in 0..200 {
        body.push_str(&format!("\"^cmd{i:03} \":\n  KEY_{i:03}: val{i}\n"));
    }
    std::fs::write(dir.path().join("lade.yml"), body).unwrap();
    let parse_started = Instant::now();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let parse_ms = parse_started.elapsed();
    let match_started = Instant::now();
    for i in 0..200 {
        let hits = config.collect(&format!("cmd{i:03} run"));
        assert!(
            hits.len() >= 2,
            "dot plus cmd{i:03} should match, got {}",
            hits.len()
        );
    }
    let match_ms = match_started.elapsed();
    assert!(
        parse_ms.as_millis() < 1000,
        "parse 201 rules took {parse_ms:?}"
    );
    assert!(
        match_ms.as_millis() < 1000,
        "200 collect calls took {match_ms:?}"
    );
}
