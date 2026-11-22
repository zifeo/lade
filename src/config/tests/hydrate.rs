use crate::config::*;
use std::collections::HashMap;
use tempfile::tempdir;

#[tokio::test]
async fn test_later_network_replaces_secret() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  DB_PORT: \"5432\"\n\"cmd\":\n  DB_PORT: kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let (vars, _, _, _) = config.collect_hydrate("cmd").await.unwrap();
    assert!(
        vars.get(&None::<std::path::PathBuf>)
            .and_then(|env| env.get("DB_PORT"))
            .is_none()
    );
    let bindings = config.collect_network_bindings("cmd", &None);
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].key, "DB_PORT");
}

#[tokio::test]
async fn test_later_secret_replaces_network() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  DB_PORT: kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432\n\"cmd\":\n  DB_PORT: \"5432\"\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let (vars, _, _, _) = config.collect_hydrate("cmd").await.unwrap();
    let env = vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_eq!(env.get("DB_PORT").unwrap(), "5432");
    assert!(config.collect_network_bindings("cmd", &None).is_empty());
}

#[tokio::test]
async fn test_collect_hydrate_rejects_numeric_non_network_key() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  1223: plain-secret\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let err = config.collect_hydrate("cmd").await.expect_err("must fail");
    assert!(
        err.to_string()
            .contains("numeric key '1223' must use a tunnel URI")
    );
}

#[tokio::test]
async fn test_collect_hydrate_child_file_overlays_parent_file() {
    let parent = tempdir().unwrap();
    let child = parent.path().join("child");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(
        parent.path().join("lade.yml"),
        "\"cmd\":\n  TOKEN: parent\n",
    )
    .unwrap();
    std::fs::write(child.join("lade.yml"), "\"cmd\":\n  TOKEN: child\n").unwrap();
    let config = LadeFile::build(child).unwrap();
    let (vars, _, _, _) = config.collect_hydrate("cmd").await.unwrap();
    let env = vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_eq!(env.get("TOKEN").unwrap(), "child");
    let plan = config.collect_secret_sources("cmd").unwrap();
    assert!(plan.overridden.contains("TOKEN"));
    assert_eq!(plan.sources.get("TOKEN").unwrap(), "child");
}

#[tokio::test]
async fn test_collect_hydrate_empty_string_is_not_cancel() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  SSH_AUTH_SOCK: \"\"\n\"^git \":\n  SSH_AUTH_SOCK: ~\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let (vars, _, _, _) = config
        .collect_hydrate("ssh -T git@github.com")
        .await
        .unwrap();
    let env = vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_eq!(env.get("SSH_AUTH_SOCK").unwrap(), "");
    let (vars, _, _, _) = config.collect_hydrate("git status").await.unwrap();
    assert!(
        vars.get(&None::<std::path::PathBuf>)
            .and_then(|env| env.get("SSH_AUTH_SOCK"))
            .is_none()
    );
}

#[tokio::test]
async fn test_collect_hydrate_later_rule_overlays_same_key() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd.*\":\n  TOKEN: parent\n\".*\":\n  TOKEN: child\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let (vars, _, _, _) = config.collect_hydrate("cmd run").await.unwrap();
    let env = vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_eq!(env.get("TOKEN").unwrap(), "child");
}

#[tokio::test]
async fn test_collect_hydrate_null_cancels_earlier_key() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  TOKEN: catch\n\"^git \":\n  TOKEN: ~\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let (vars, _, _, _) = config.collect_hydrate("git status").await.unwrap();
    assert!(
        vars.get(&None::<std::path::PathBuf>)
            .and_then(|env| env.get("TOKEN"))
            .is_none()
    );
    let (vars, _, _, _) = config
        .collect_hydrate("ssh -T git@github.com")
        .await
        .unwrap();
    let env = vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_eq!(env.get("TOKEN").unwrap(), "catch");
}

#[tokio::test]
async fn test_collect_hydrate_allows_identical_duplicates() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd.*\":\n  TOKEN: same\n\".*\":\n  TOKEN: same\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let (vars, _, _, _) = config.collect_hydrate("cmd run").await.unwrap();
    let env = vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_eq!(env.get("TOKEN").unwrap(), "same");
}

#[tokio::test]
async fn test_collect_hydrate_interpolates_private_binding_without_emitting_it() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  .TOKEN: token\n  Authorization: \"Bearer ${TOKEN}\"\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let (vars, _, maskable, _) = config.collect_hydrate("cmd").await.unwrap();
    let env = vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_eq!(env.get("Authorization"), Some(&"Bearer token".to_string()));
    assert!(!env.contains_key("TOKEN"));
    assert!(!maskable.contains("TOKEN"));
}

#[tokio::test]
async fn test_collect_hydrate_rejects_public_private_collision() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  TOKEN: public\n  .TOKEN: private\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let err = config.collect_hydrate("cmd").await.unwrap_err();
    assert!(
        err.to_string()
            .contains("binding 'TOKEN' is declared both public and private")
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_collect_hydrate_injects_dependencies_into_shell_provider() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        r#""cmd":
  user: demo-user
  .password: demo-password
  Authorization: 'sh://printf "Basic %s" "$(printf "%s:%s" "${user}" "${.password}" | base64 | tr -d "\n")"'
"#,
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let (vars, _, _, _) = config.collect_hydrate("cmd").await.unwrap();
    let env = vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_eq!(
        env.get("Authorization"),
        Some(&"Basic ZGVtby11c2VyOmRlbW8tcGFzc3dvcmQ=".to_string())
    );
    assert_eq!(env.get("user"), Some(&"demo-user".to_string()));
    assert!(!env.contains_key("password"));
}

#[test]
fn test_pre_event_work_resolves_user_maps_and_private_keys() {
    use crate::event::match_tree_from;
    use crate::ticket::TicketSecret;

    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  API_TOKEN:\n    alice: raw://alice-token\n    \".\": raw://default-token\n  .SESSION: raw://session\n  DB_PORT:\n    alice: kubectl://a:6443/example-cluster/dev/service/postgres/5432\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let rules = config
        .collect_for_with_pattern("cmd", Audience::Human)
        .unwrap();
    let work = Config::pre_event_work(&rules, &Some("alice".to_string())).unwrap();
    assert!(work.disclaimers.is_empty());
    assert_eq!(work.secrets.len(), 2);
    let session = work
        .secrets
        .iter()
        .find(|secret| secret.key == "SESSION")
        .expect("private binding");
    assert_eq!(
        session,
        &TicketSecret {
            key: "SESSION".to_string(),
            source: "raw://session".to_string(),
            private: true,
            output: None,
            cwd: dir.path().to_path_buf(),
            op_sa: None,
        }
    );
    let token = work
        .secrets
        .iter()
        .find(|secret| secret.key == "API_TOKEN")
        .expect("user-mapped secret");
    assert_eq!(token.source, "raw://alice-token");
    assert!(!token.private);
    assert_eq!(work.network.len(), 1);
    assert_eq!(work.network[0].key, "DB_PORT");
    assert!(work.network[0].uri.starts_with("kubectl://"));
    assert_eq!(
        work.matches,
        match_tree_from(&rules, &Some("alice".to_string()))
    );
    assert!(!work.log);
}

#[test]
fn match_tree_reads_lock_beside_yaml() {
    use crate::event::match_tree_from;

    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^terraform:\n  TF_VAR_FOO: op://v/i/f\n  tofu: mise://aqua/opentofu/opentofu@1.8.2\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.lock"),
        "[[tools.op]]\nversion = \"2.31.0\"\n\n[[tools.tofu]]\nversion = \"1.8.2\"\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let rules = config
        .collect_for_with_pattern("terraform plan", Audience::Human)
        .unwrap();
    let matches = match_tree_from(&rules, &None);
    let text = matches.to_string();
    assert!(text.contains("\"family\":\"secret\""), "{text}");
    assert!(text.contains("\"family\":\"package\""), "{text}");
    assert!(text.contains("\"package\":\"op\""), "{text}");
    assert!(text.contains("\"version\":\"2.31.0\""), "{text}");
    assert!(text.contains("\"package\":\"tofu\""), "{text}");
    assert!(text.contains("\"version\":\"1.8.2\""), "{text}");
}

#[test]
fn match_tree_reads_a_lock_keyed_by_tool_id() {
    use crate::event::match_tree_from;

    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^cargo:\n  TOKEN: op://v/i/f\n  cargo: mise://core/rust@1.96.0\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.lock"),
        "[[tools.\"aqua:1password/cli\"]]\nversion = \"2.39.0\"\nbackend = \"aqua:1password/cli\"\n\n[[tools.rust]]\nversion = \"1.96.0\"\nbackend = \"core:rust\"\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let rules = config
        .collect_for_with_pattern("cargo build", Audience::Human)
        .unwrap();
    let text = match_tree_from(&rules, &None).to_string();
    assert!(text.contains("\"package\":\"op\""), "{text}");
    assert!(text.contains("\"version\":\"2.39.0\""), "{text}");
    assert!(text.contains("\"package\":\"cargo\""), "{text}");
    assert!(text.contains("\"version\":\"1.96.0\""), "{text}");
}

#[test]
fn match_tree_leaves_out_a_lock_version_the_pin_disagrees_with() {
    use crate::event::match_tree_from;

    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yaml"),
        "^terraform:\n  tofu: mise://aqua/opentofu/opentofu@1.8.2\n",
    )
    .unwrap();
    std::fs::write(
        dir.path().join("lade.lock"),
        "[[tools.tofu]]\nversion = \"1.8.1\"\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let rules = config
        .collect_for_with_pattern("terraform plan", Audience::Human)
        .unwrap();
    let text = match_tree_from(&rules, &None).to_string();
    assert!(text.contains("\"package\":\"tofu\""), "{text}");
    assert!(!text.contains("\"version\""), "{text}");
}

#[tokio::test]
async fn test_hydrate_work_plain_source() {
    use crate::ticket::TicketSecret;

    let dir = tempdir().unwrap();
    let cwd = dir.path().to_path_buf();
    let secrets = vec![TicketSecret {
        key: "PLAIN".to_string(),
        source: "hello".to_string(),
        private: false,
        output: None,
        cwd: cwd.clone(),
        op_sa: None,
    }];
    let (vars, sources, ..) = Config::hydrate_work_on_group(&secrets, &HashMap::new(), |_| {})
        .await
        .unwrap();
    let env = vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_eq!(env.get("PLAIN").unwrap(), "hello");
    assert_eq!(sources.get("PLAIN").unwrap(), "hello");
}

#[tokio::test]
async fn test_hydrate_work_private_and_file_output() {
    use crate::ticket::TicketSecret;

    let dir = tempdir().unwrap();
    let cwd = dir.path().to_path_buf();
    let secrets = vec![
        TicketSecret {
            key: "PUBLIC".to_string(),
            source: "a".to_string(),
            private: false,
            output: Some(cwd.join("out.json")),
            cwd: cwd.clone(),
            op_sa: None,
        },
        TicketSecret {
            key: "HIDDEN".to_string(),
            source: "b".to_string(),
            private: true,
            output: None,
            cwd: cwd.clone(),
            op_sa: None,
        },
    ];
    let (vars, _, maskable, ..) = Config::hydrate_work_on_group(&secrets, &HashMap::new(), |_| {})
        .await
        .unwrap();
    let env = vars.get(&Some(cwd.join("out.json"))).unwrap();
    assert_eq!(env.get("PUBLIC").unwrap(), "a");
    assert!(!env.contains_key("HIDDEN"));
    assert!(!maskable.contains("HIDDEN"));
}

#[tokio::test]
async fn hydrate_work_notifies_shell_and_raw_apart() {
    use crate::ticket::TicketSecret;
    use std::sync::{Arc, Mutex};

    let dir = tempdir().unwrap();
    let cwd = dir.path().to_path_buf();
    let secrets = vec![
        TicketSecret {
            key: "PLAIN".to_string(),
            source: "hello".to_string(),
            private: false,
            output: None,
            cwd: cwd.clone(),
            op_sa: None,
        },
        TicketSecret {
            key: "SOCKET".to_string(),
            source: r#"sh://printf %s ready"#.to_string(),
            private: false,
            output: None,
            cwd,
            op_sa: None,
        },
    ];
    let waves = Arc::new(Mutex::new(Vec::<Vec<String>>::new()));
    let seen = waves.clone();
    Config::hydrate_work_on_group(&secrets, &HashMap::new(), move |done| {
        let mut keys: Vec<String> = done.iter().cloned().collect();
        keys.sort();
        seen.lock().unwrap().push(keys);
    })
    .await
    .unwrap();
    let waves = waves.lock().unwrap().clone();
    assert_eq!(waves.len(), 2, "{waves:?}");
    assert!(waves.iter().any(|wave| wave == &["PLAIN".to_string()]));
    assert!(waves.iter().any(|wave| wave == &["SOCKET".to_string()]));
}

#[tokio::test]
async fn ticket_hydrate_keeps_per_rule_op_sa() {
    let parent = tempdir().unwrap();
    let child = parent.path().join("app");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(
        parent.path().join("lade.yml"),
        "\"cmd\":\n  \".\":\n    1password_service_account: raw://token-a\n  A: sh://printenv OP_SERVICE_ACCOUNT_TOKEN\n",
    )
    .unwrap();
    std::fs::write(
        child.join("lade.yml"),
        "\"cmd\":\n  \".\":\n    1password_service_account: raw://token-b\n  B: sh://printenv OP_SERVICE_ACCOUNT_TOKEN\n",
    )
    .unwrap();
    let config = LadeFile::build(child).unwrap();
    let (live_vars, _, _, _) = config.collect_hydrate("cmd").await.unwrap();
    let live = live_vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_ne!(live.get("A"), live.get("B"), "{live:?}");

    let rules = config
        .collect_for_with_pattern("cmd", Audience::Human)
        .unwrap();
    let work = Config::pre_event_work(&rules, &None).unwrap();
    let a = work
        .secrets
        .iter()
        .find(|secret| secret.key == "A")
        .expect("A");
    let b = work
        .secrets
        .iter()
        .find(|secret| secret.key == "B")
        .expect("B");
    assert_eq!(a.op_sa.as_deref(), Some("raw://token-a"));
    assert_eq!(b.op_sa.as_deref(), Some("raw://token-b"));

    let (ticket_vars, ..) = Config::hydrate_work_on_group(&work.secrets, &HashMap::new(), |_| {})
        .await
        .unwrap();
    let ticket = ticket_vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_eq!(ticket.get("A"), live.get("A"), "{ticket:?} {live:?}");
    assert_eq!(ticket.get("B"), live.get("B"), "{ticket:?} {live:?}");
}

#[tokio::test]
async fn ticket_hydrate_gives_no_account_to_a_rule_without_one() {
    let parent = tempdir().unwrap();
    let child = parent.path().join("app");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(
        parent.path().join("lade.yml"),
        "\"cmd\":\n  \".\":\n    1password_service_account: token-a\n  A: sh://printf %s \"${OP_SERVICE_ACCOUNT_TOKEN-none}\"\n",
    )
    .unwrap();
    std::fs::write(
        child.join("lade.yml"),
        "\"cmd\":\n  B: sh://printf %s \"${OP_SERVICE_ACCOUNT_TOKEN-none}\"\n",
    )
    .unwrap();
    let config = LadeFile::build(child).unwrap();
    let rules = config
        .collect_for_with_pattern("cmd", Audience::Human)
        .unwrap();
    let work = Config::pre_event_work(&rules, &None).unwrap();
    let (vars, ..) = Config::hydrate_work_on_group(&work.secrets, &HashMap::new(), |_| {})
        .await
        .unwrap();
    let env = vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_eq!(env.get("A").map(String::as_str), Some("token-a"), "{env:?}");
    assert_eq!(env.get("B").map(String::as_str), Some("none"), "{env:?}");
}

#[tokio::test]
async fn private_tunnel_binding_fails() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  .PORT: kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let err = config.collect_hydrate("cmd").await.unwrap_err();
    assert!(err.to_string().contains("'.PORT'"), "{err:#}");
    let rules = config
        .collect_for_with_pattern("cmd", Audience::Human)
        .unwrap();
    let err = Config::pre_event_work(&rules, &None)
        .err()
        .expect("refused");
    assert!(err.to_string().contains("'.PORT'"), "{err:#}");
}

#[tokio::test]
async fn private_package_binding_fails() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        "\"cmd\":\n  .NODE: mise://aqua/jqlang/jq@1.7.1\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let err = config.collect_hydrate("cmd").await.unwrap_err();
    assert!(err.to_string().contains("'.NODE'"), "{err:#}");
}

fn ticket_secret(cwd: &std::path::Path, key: &str, source: &str) -> crate::ticket::TicketSecret {
    crate::ticket::TicketSecret {
        key: key.to_string(),
        source: source.to_string(),
        private: false,
        output: None,
        cwd: cwd.to_path_buf(),
        op_sa: None,
    }
}

#[tokio::test]
async fn skipped_binding_still_feeds_its_dependent() {
    let dir = tempdir().unwrap();
    let secrets = vec![
        // Fetching A would fail: only the hub's value may feed B and C.
        ticket_secret(dir.path(), "A", "sh://exit 1"),
        ticket_secret(dir.path(), "B", "Bearer ${A}"),
        ticket_secret(dir.path(), "C", "sh://printf %s \"$A\""),
    ];
    let seed = HashMap::from([("A".to_string(), "served".to_string())]);
    let (vars, _, maskable, ..) = Config::hydrate_work_on_group(&secrets, &seed, |_| {})
        .await
        .unwrap();
    let env = vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_eq!(env.get("A").map(String::as_str), Some("served"));
    assert_eq!(env.get("B").map(String::as_str), Some("Bearer served"));
    assert_eq!(env.get("C").map(String::as_str), Some("served"));
    assert!(maskable.contains("A"));
    assert!(maskable.contains("B"));
}

#[tokio::test]
async fn a_failed_group_fails_the_whole_hydrate() {
    let dir = tempdir().unwrap();
    // The raw value and the shell secret hydrate in separate groups.
    let secrets = vec![
        ticket_secret(dir.path(), "OK", "fine"),
        ticket_secret(dir.path(), "BAD", "sh://exit 1"),
    ];
    assert!(
        Config::hydrate_work_on_group(&secrets, &HashMap::new(), |_| {})
            .await
            .is_err()
    );
}

#[tokio::test]
async fn child_binding_references_parent_private_binding() {
    let parent = tempdir().unwrap();
    let child = parent.path().join("app");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(parent.path().join("lade.yml"), "\"cmd\":\n  .TOKEN: t\n").unwrap();
    std::fs::write(
        child.join("lade.yml"),
        "\"cmd\":\n  AUTH: \"Bearer ${TOKEN}\"\n",
    )
    .unwrap();
    let config = LadeFile::build(child).unwrap();
    let (vars, ..) = config.collect_hydrate("cmd").await.unwrap();
    let env = vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_eq!(env.get("AUTH").map(String::as_str), Some("Bearer t"));
    assert!(!env.contains_key("TOKEN"));
}

async fn hydrate_for(config: &Config, user: &str) -> HashMap<String, String> {
    let rules = config.collect("cmd").unwrap();
    let (vars, ..) = config
        .hydrate_rules(&rules, &Some(user.to_string()))
        .await
        .unwrap();
    vars.get(&None::<std::path::PathBuf>)
        .cloned()
        .unwrap_or_default()
}

#[tokio::test]
async fn user_map_tilde_cancels_parent_value_for_that_user() {
    let parent = tempdir().unwrap();
    let child = parent.path().join("app");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(parent.path().join("lade.yml"), "\"cmd\":\n  KEY: parent\n").unwrap();
    std::fs::write(
        child.join("lade.yml"),
        "\"cmd\":\n  KEY:\n    alice: ~\n    bob: b\n",
    )
    .unwrap();
    let config = LadeFile::build(child).unwrap();
    assert!(!hydrate_for(&config, "alice").await.contains_key("KEY"));
    assert_eq!(
        hydrate_for(&config, "bob")
            .await
            .get("KEY")
            .map(String::as_str),
        Some("b")
    );
}

#[tokio::test]
async fn other_audience_body_takes_no_part_in_overlay() {
    let parent = tempdir().unwrap();
    let child = parent.path().join("app");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(parent.path().join("lade.yml"), "\"cmd\":\n  KEY: parent\n").unwrap();
    std::fs::write(
        child.join("lade.yml"),
        "\"cmd\":\n  - \".\":\n      when: agent\n    KEY: ~\n",
    )
    .unwrap();
    let config = LadeFile::build(child).unwrap();
    let mut env = HashMap::new();
    for audience in [Audience::Human, Audience::Agent] {
        let rules = config.collect_for("cmd", audience).unwrap();
        let (vars, ..) = config.hydrate_rules(&rules, &None).await.unwrap();
        let vars = vars
            .get(&None::<std::path::PathBuf>)
            .cloned()
            .unwrap_or_default();
        env.insert(format!("{audience:?}"), vars);
    }
    assert_eq!(env["Human"].get("KEY").map(String::as_str), Some("parent"));
    assert!(!env["Agent"].contains_key("KEY"));
}

#[tokio::test]
async fn user_map_without_entry_keeps_parent_value() {
    let parent = tempdir().unwrap();
    let child = parent.path().join("app");
    std::fs::create_dir(&child).unwrap();
    std::fs::write(parent.path().join("lade.yml"), "\"cmd\":\n  KEY: parent\n").unwrap();
    std::fs::write(child.join("lade.yml"), "\"cmd\":\n  KEY:\n    alice: x\n").unwrap();
    let config = LadeFile::build(child).unwrap();
    assert_eq!(
        hydrate_for(&config, "bob")
            .await
            .get("KEY")
            .map(String::as_str),
        Some("parent")
    );
}

#[tokio::test]
async fn private_masked_binding_is_returned_and_maskable() {
    let dir = tempdir().unwrap();
    let json = dir.path().join("s.json");
    std::fs::write(&json, r#"{"k":"tok-secret"}"#).unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        format!(
            "\"cmd\":\n  .TOKEN: \"file://{}?query=.k\"\n  AUTH: \"Bearer ${{TOKEN}}\"\n",
            json.display()
        ),
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let rules = config
        .collect_for_with_pattern("cmd", Audience::Human)
        .unwrap();
    let work = Config::pre_event_work(&rules, &None).unwrap();
    let (vars, _, maskable, _, private) =
        Config::hydrate_work_on_group(&work.secrets, &HashMap::new(), |_| {})
            .await
            .unwrap();
    let env = vars.get(&None::<std::path::PathBuf>).unwrap();
    assert_eq!(
        env.get("AUTH").map(String::as_str),
        Some("Bearer tok-secret")
    );
    assert!(!env.contains_key("TOKEN"));
    assert_eq!(private.get("TOKEN").map(String::as_str), Some("tok-secret"));
    assert!(maskable.contains("TOKEN"), "{maskable:?}");
    assert!(maskable.contains("AUTH"), "{maskable:?}");
}

fn plan_error(yaml: &str) -> String {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("lade.yml"), yaml).unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let rules = config
        .collect_for_with_pattern("cmd", Audience::Human)
        .unwrap();
    format!(
        "{:#}",
        Config::pre_event_work(&rules, &None)
            .err()
            .expect("refused")
    )
}

#[test]
fn a_private_secret_beside_a_public_tunnel_of_its_name_fails() {
    let tunnel = "kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432";
    let later_private = plan_error(&format!(
        ".:\n  DB_PORT: {tunnel}\n\"cmd\":\n  .DB_PORT: secret\n"
    ));
    assert!(
        later_private.contains("binding 'DB_PORT' is declared both public and private"),
        "{later_private}"
    );
    let later_public = plan_error(&format!(
        ".:\n  .DB_PORT: secret\n\"cmd\":\n  DB_PORT: {tunnel}\n"
    ));
    assert!(
        later_public.contains("binding 'DB_PORT' is declared both public and private"),
        "{later_public}"
    );
}

#[test]
fn a_cancelled_public_tunnel_leaves_room_for_a_private_secret() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  DB_PORT: kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432\n\"c\":\n  DB_PORT: ~\n\"cmd\":\n  .DB_PORT: secret\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let rules = config
        .collect_for_with_pattern("cmd", Audience::Human)
        .unwrap();
    let work = Config::pre_event_work(&rules, &None).unwrap();
    assert!(work.network.is_empty());
    assert!(work.secrets.iter().any(|s| s.key == "DB_PORT" && s.private));
}

#[test]
fn a_missing_reference_fails_the_plan() {
    let err = plan_error("\"cmd\":\n  AUTH: \"Bearer ${TOKEN}\"\n");
    assert!(
        err.contains("binding 'AUTH' references missing dependency 'TOKEN'"),
        "{err}"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn a_missing_reference_fails_before_the_service_account_is_fetched() {
    let dir = tempdir().unwrap();
    let fetched = dir.path().join("fetched");
    std::fs::write(
        dir.path().join("lade.yml"),
        format!(
            "\"cmd\":\n  \".\":\n    1password_service_account: \"sh://touch {} && echo token\"\n  AUTH: \"Bearer ${{TOKEN}}\"\n",
            fetched.display()
        ),
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    let err = config.collect_hydrate("cmd").await.unwrap_err();
    assert!(
        format!("{err:#}").contains("references missing dependency 'TOKEN'"),
        "{err:#}"
    );
    assert!(!fetched.exists());
}

/// A body's map has no order: each build reseeds it, so one that read
/// `TOKEN: ~` after `.TOKEN` would fail some builds.
#[tokio::test]
async fn a_cancel_beside_a_value_of_its_name_cancels_only_the_parent_value() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  TOKEN: parent\n\"cmd\":\n  TOKEN: ~\n  .TOKEN: child\n  AUTH: Bearer ${TOKEN}\n",
    )
    .unwrap();
    for _ in 0..32 {
        let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
        let (vars, _, _, _) = config.collect_hydrate("cmd").await.unwrap();
        let env = vars.get(&None::<std::path::PathBuf>).unwrap();
        assert_eq!(env.get("AUTH").map(String::as_str), Some("Bearer child"));
        assert!(!env.contains_key("TOKEN"), "{env:?}");
        let rules = config
            .collect_for_with_pattern("cmd", Audience::Human)
            .unwrap();
        let work = Config::pre_event_work(&rules, &None).unwrap();
        let token = work
            .secrets
            .iter()
            .find(|secret| secret.key == "TOKEN")
            .expect("planned");
        assert!(token.private);
    }
}

#[test]
fn a_private_cancel_removes_a_public_binding_of_its_name_in_every_family() {
    let dir = tempdir().unwrap();
    std::fs::write(
        dir.path().join("lade.yml"),
        ".:\n  DB_PORT: kubectl://k8s.example.com:6443/example-cluster/dev/service/postgres/5432\n  jq: mise://aqua/jqlang/jq@1.7.1\n  TOKEN: parent\n\"cmd\":\n  .DB_PORT: ~\n  .jq: ~\n  .TOKEN: ~\n",
    )
    .unwrap();
    let config = LadeFile::build(dir.path().to_path_buf()).unwrap();
    assert!(config.collect_network_bindings("cmd", &None).is_empty());
    assert!(
        config
            .pins_for_command("cmd", &None, Audience::Human)
            .unwrap()
            .is_empty()
    );
    let rules = config
        .collect_for_with_pattern("cmd", Audience::Human)
        .unwrap();
    let work = Config::pre_event_work(&rules, &None).unwrap();
    assert!(work.secrets.is_empty(), "{:?}", work.secrets);
    assert!(work.network.is_empty());
}
