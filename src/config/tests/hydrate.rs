use crate::config::*;
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
            .contains("numeric key '1223' must use a network URI")
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
    let rules = config.collect_for_with_pattern("cmd", Audience::Human);
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
    assert!(work.op_sa.is_none());
    assert!(!work.log);
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
    }];
    let (vars, sources, _, _) = Config::hydrate_work(&secrets, None).await.unwrap();
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
        },
        TicketSecret {
            key: "HIDDEN".to_string(),
            source: "b".to_string(),
            private: true,
            output: None,
            cwd: cwd.clone(),
        },
    ];
    let (vars, _, maskable, _) = Config::hydrate_work(&secrets, None).await.unwrap();
    let env = vars.get(&Some(cwd.join("out.json"))).unwrap();
    assert_eq!(env.get("PUBLIC").unwrap(), "a");
    assert!(!env.contains_key("HIDDEN"));
    assert!(!maskable.contains("HIDDEN"));
}
