use super::*;
use crate::providers::{Transport, Warnings, fake_cli};
use std::path::Path;
use tempfile::tempdir;

fn path_env(dir: &tempfile::TempDir) -> HashMap<String, String> {
    HashMap::from([(
        "PATH".to_string(),
        dir.path().to_string_lossy().into_owned(),
    )])
}

#[test]
fn test_add_routing() {
    let mut p = AwsSm::new();
    assert!(p.add("awssm://eu-west-1/name".to_string()).is_ok());
    assert!(p.add("awssm:///name".to_string()).is_err());
    assert!(p.add("vault://h/m/k/f".to_string()).is_err());
    assert_eq!(p.transport(), Transport::Cli);
}

#[tokio::test]
#[cfg(unix)]
async fn test_batch_and_query() {
    let fake_bin = tempdir().unwrap();
    let calls = fake_bin.path().join("calls");
    fake_cli(
        &fake_bin,
        "aws",
        &format!(
            "echo x >> '{}'; echo '{{\"SecretString\":\"{{\\\"password\\\":\\\"s3cret\\\",\\\"user\\\":\\\"app\\\"}}\"}}'",
            calls.display()
        ),
    );
    let mut p = AwsSm::new();
    p.add("awssm://us-east-1/myapp/db?query=.password".to_string())
        .unwrap();
    p.add("awssm://us-east-1/myapp/db?query=.user".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get("awssm://us-east-1/myapp/db?query=.password")
            .unwrap(),
        "s3cret"
    );
    assert_eq!(
        result
            .get("awssm://us-east-1/myapp/db?query=.user")
            .unwrap(),
        "app"
    );
    let calls = std::fs::read_to_string(fake_bin.path().join("calls")).unwrap();
    assert_eq!(calls.matches('x').count(), 1);
}

#[tokio::test]
#[cfg(unix)]
async fn test_version_uses_get() {
    let fake_bin = tempdir().unwrap();
    fake_cli(
        &fake_bin,
        "aws",
        r#"
for a in "$@"; do
  if [ "$a" = "--version-id" ]; then
    echo '{"SecretString":"pinned"}'
    exit 0
  fi
done
echo missing version >&2
exit 1
"#,
    );
    let mut p = AwsSm::new();
    p.add("awssm://us-east-1/db?version=abc".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result.get("awssm://us-east-1/db?version=abc").unwrap(),
        "pinned"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_arn_alias_lookup() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "aws", r#"echo '{"SecretString":"from-arn"}'"#);
    let mut p = AwsSm::new();
    p.add("awssm://us-east-1/arn:aws:secretsmanager:us-east-1:1:secret:db".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get("awssm://us-east-1/arn:aws:secretsmanager:us-east-1:1:secret:db")
            .unwrap(),
        "from-arn"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_fail_closed_on_missing() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "aws", "echo missing >&2; exit 1");
    let mut p = AwsSm::new();
    p.add("awssm://us-east-1/missing".to_string()).unwrap();
    let err = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("missing"), "{err}");
}
