use crate::config::*;
use tempfile::tempdir;

#[test]
fn test_collect_dot_matches_any_non_empty_command() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yml"), ".:\n  KEY: val\n").unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert_eq!(config.collect("git status").len(), 1);
    assert_eq!(config.collect("ssh -T git@github.com").len(), 1);
    assert!(config.collect("").is_empty());
}

#[test]
fn test_collect_for_filters_when() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  \".\":\n    when: agent\n  SOCK: agent-sock\n\"^git \":\n  \".\":\n    when: human\n  SOCK: human-sock\n\"echo\":\n  SOCK: always-sock\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let agent = config.collect_for("git status", Audience::Agent);
    assert_eq!(agent.len(), 1);
    assert!(agent[0].1.secrets.contains_key("SOCK"));
    assert_eq!(agent[0].1.config.as_ref().unwrap().when, RuleWhen::Agent);
    let human = config.collect_for("git status", Audience::Human);
    assert_eq!(human.len(), 1);
    assert_eq!(human[0].1.config.as_ref().unwrap().when, RuleWhen::Human);
    let echo_agent = config.collect_for("echo hi", Audience::Agent);
    assert_eq!(echo_agent.len(), 2);
    let echo_human = config.collect_for("echo hi", Audience::Human);
    assert_eq!(echo_human.len(), 1);
    assert!(echo_human[0].1.config.is_none());
}

#[test]
fn test_log_enabled_absent_is_off() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yml"), "\"cmd\":\n  KEY: val\n").unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert!(!Config::log_enabled(
        &config.collect_for("cmd", Audience::Human)
    ));
}

#[test]
fn test_log_enabled_overlay_last_explicit_wins() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  .:\n    log: true\n\"^git status\":\n  .:\n    log: false\n\"^npm run deploy\":\n  .:\n    log: true\n  API_TOKEN: op://prod/api/credential\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert!(Config::log_enabled(
        &config.collect_for("echo hi", Audience::Human)
    ));
    assert!(!Config::log_enabled(
        &config.collect_for("git status", Audience::Human)
    ));
    assert!(Config::log_enabled(
        &config.collect_for("npm run deploy", Audience::Human)
    ));
}

#[test]
fn test_log_enabled_parent_then_child() {
    let parent = tempdir().unwrap();
    std::fs::write(parent.path().join("lade.yml"), ".:\n  .:\n    log: true\n").unwrap();
    let child = parent.path().join("app");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(
        child.join("lade.yml"),
        "\"^git status\":\n  .:\n    log: false\n\"^npm run deploy\":\n  .:\n    log: true\n  API_TOKEN: raw://x\n",
    )
    .unwrap();
    let config = LadeFile::build(child).unwrap();
    assert!(Config::log_enabled(
        &config.collect_for("echo hi", Audience::Human)
    ));
    assert!(!Config::log_enabled(
        &config.collect_for("git status", Audience::Human)
    ));
    assert!(Config::log_enabled(
        &config.collect_for("npm run deploy", Audience::Human)
    ));
}

#[test]
fn test_log_enabled_dot_then_later_secret_keeps_log() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  .:\n    log: true\n\"^echo \":\n  API_TOKEN: raw://x\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert!(Config::log_enabled(
        &config.collect_for("echo hi", Audience::Human)
    ));
}

#[test]
fn test_collect_for_default_when_is_always() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yml"), "\"cmd\":\n  KEY: val\n").unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert_eq!(config.collect_for("cmd", Audience::Agent).len(), 1);
    assert_eq!(config.collect_for("cmd", Audience::Human).len(), 1);
}

#[test]
fn test_collect_exact_match() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"terraform plan\":\n  KEY: val\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert_eq!(config.collect("terraform plan").len(), 1);
}

#[test]
fn test_collect_regex_match() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"terraform.*\":\n  KEY: val\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert_eq!(config.collect("terraform plan").len(), 1);
    assert_eq!(config.collect("terraform apply").len(), 1);
    assert_eq!(config.collect("other command").len(), 0);
}

#[test]
fn test_collect_disclaimers_multiple_and_deduped() {
    let dir = tempdir().unwrap();
    // Two rules match "deploy prod": one unique disclaimer each, plus a
    // duplicate shared text that must appear only once.
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"deploy\":\n  \".\":\n    disclaimer: \"Shared warning.\"\n  A: a\n\
         \"prod\":\n  \".\":\n    disclaimer: \"Shared warning.\"\n  B: b\n\
         \"deploy prod\":\n  \".\":\n    disclaimer: \"Extra warning.\"\n  C: c\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let disclaimers = config.collect_disclaimers("deploy prod");
    assert_eq!(disclaimers.len(), 2);
    assert!(disclaimers.contains(&"Shared warning.".to_string()));
    assert!(disclaimers.contains(&"Extra warning.".to_string()));
}

#[test]
fn test_collect_no_match() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yml"), "\"specific\":\n  KEY: val\n").unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert!(config.collect("other").is_empty());
}

// Shell hooks run `build` + `collect` on EVERY command, and most commands do
// not match. This guards that common hot path against gross regressions; the
// budget is generous (CI runners vary wildly) but still catches a 10-100x
// slowdown. Vault resolution is intentionally excluded (it is rare and
// network-bound).
#[test]
fn hot_path_build_and_no_match_is_fast() {
    use std::time::{Duration, Instant};
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"terraform .*\":\n  AWS_TOKEN: op://vault/item/field\n\"kubectl .*\":\n  KUBE_TOKEN: op://vault/item/field\n",
    )
    .unwrap();
    let path = dir.path().to_path_buf();

    for _ in 0..50 {
        let config = LadeFile::build(path.clone()).unwrap();
        assert!(config.collect("git status").is_empty());
    }

    let iters = 1000u32;
    let start = Instant::now();
    for _ in 0..iters {
        let config = LadeFile::build(path.clone()).unwrap();
        let _ = config.collect("git status --porcelain");
    }
    let per_iter = start.elapsed() / iters;

    assert!(
        per_iter < Duration::from_millis(5),
        "hot path regressed: {per_iter:?} per build+no-match (budget 5ms)"
    );
}

#[test]
fn test_collect_multiple_rules_match() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd.*\":\n  KEY1: val1\n\".*\":\n  KEY2: val2\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert_eq!(config.collect("cmd anything").len(), 2);
}

#[test]
fn test_collect_disclaimers() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"terraform destroy\":\n  \".\":\n    disclaimer: \"This will destroy infrastructure.\"\n  KEY: val\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let disclaimers = config.collect_disclaimers("terraform destroy");
    assert_eq!(disclaimers.len(), 1);
    assert_eq!(disclaimers[0], "This will destroy infrastructure.");
    assert!(config.collect_disclaimers("terraform plan").is_empty());
}

#[test]
fn lookahead_excludes_help_flag() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"^terraform apply(?!.*--help)\":\n  TOKEN: val\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert_eq!(config.collect("terraform apply").len(), 1);
    assert_eq!(config.collect("terraform apply -auto-approve").len(), 1);
    assert!(config.collect("terraform apply --help").is_empty());
}

#[test]
fn lookahead_keeps_easy_overlay_order() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"^terraform\":\n  BASE: val\n\"^terraform apply(?!.*--help)\":\n  EXTRA: val\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let apply = config.collect("terraform apply");
    assert_eq!(apply.len(), 2);
    assert!(apply[0].1.secrets.contains_key("BASE"));
    assert!(apply[1].1.secrets.contains_key("EXTRA"));
    let help = config.collect("terraform apply --help");
    assert_eq!(help.len(), 1);
    assert!(help[0].1.secrets.contains_key("BASE"));
}

#[test]
fn test_log_on_walk_last_explicit_wins_across_non_matching_rules() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  .:\n    log: true\n\"^git status\":\n  .:\n    log: false\n\"^npm run deploy\":\n  .:\n    log: true\n  API_TOKEN: op://prod/api/credential\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert!(config.log_on_walk());
    let child = dir.path().join("app");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(
        child.join("lade.yml"),
        "\"^echo \":\n  .:\n    log: false\n  KEY: raw://x\n",
    )
    .unwrap();
    let config = LadeFile::build(child).unwrap();
    assert!(!config.log_on_walk());
}

#[test]
fn test_log_only_dot_does_not_need_wrap() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yml"), ".:\n  .:\n    log: true\n").unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let patterned = config.collect_for_with_pattern("gcloud auth list", Audience::Agent);
    assert_eq!(patterned.len(), 1);
    let work = Config::pre_event_work(&patterned, &None).unwrap();
    assert!(work.log);
    assert!(!work.needs_inject());
    assert!(!Config::needs_wrap(
        &work,
        "gcloud auth list",
        patterned.iter().map(|(_, _, rule)| rule),
        &None,
    ));
}

#[test]
fn test_secret_and_disclaimer_and_pin_need_wrap() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"^echo\":\n  KEY: val\n\"^warn\":\n  .:\n    disclaimer: Danger\n\"^jq\":\n  jq: \"core:jq@1.7.1\"\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    for command in ["echo hi", "warn me", "jq --version"] {
        let patterned = config.collect_for_with_pattern(command, Audience::Agent);
        let work = Config::pre_event_work(&patterned, &None).unwrap();
        assert!(
            Config::needs_wrap(
                &work,
                command,
                patterned.iter().map(|(_, _, rule)| rule),
                &None,
            ),
            "{command}"
        );
    }
}
