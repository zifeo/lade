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
    let mut p = AzureKv::new();
    assert!(p.add("azurekv://eng/db".to_string()).is_ok());
    assert!(
        p.add("azurekv://eng.vault.azure.net/db".to_string())
            .is_ok()
    );
    assert!(
        p.add("azurekv://eng.vault.usgovcloudapi.net/db".to_string())
            .is_ok()
    );
    assert!(p.add("azurekv://eng.vault.azure.cn/db".to_string()).is_ok());
    assert!(p.add("azurekv://eng.example.com/db".to_string()).is_err());
    assert!(p.add("vault://h/m/k/f".to_string()).is_err());
    assert_eq!(p.transport(), Transport::Cli);
}

#[tokio::test]
#[cfg(unix)]
async fn test_parallel_gets() {
    let fake_bin = tempdir().unwrap();
    fake_cli(
        &fake_bin,
        "az",
        r#"
name=""
prev=""
for a in "$@"; do
  if [ "$prev" = "--name" ]; then name="$a"; fi
  prev="$a"
done
if [ "$name" = "db" ]; then echo '{"value":"one"}'; exit 0; fi
if [ "$name" = "api" ]; then echo '{"value":"two"}'; exit 0; fi
echo missing >&2
exit 1
"#,
    );
    let mut p = AzureKv::new();
    p.add("azurekv://eng/db".to_string()).unwrap();
    p.add("azurekv://eng/api".to_string()).unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(result.get("azurekv://eng/db").unwrap(), "one");
    assert_eq!(result.get("azurekv://eng/api").unwrap(), "two");
}

#[tokio::test]
#[cfg(unix)]
async fn test_dedupe_and_query() {
    let fake_bin = tempdir().unwrap();
    let calls = fake_bin.path().join("calls");
    fake_cli(
        &fake_bin,
        "az",
        &format!(
            "echo x >> '{}'; echo '{{\"value\":\"{{\\\"password\\\":\\\"s3cret\\\",\\\"user\\\":\\\"app\\\"}}\"}}'",
            calls.display()
        ),
    );
    let mut p = AzureKv::new();
    p.add("azurekv://eng/db?query=.password".to_string())
        .unwrap();
    p.add("azurekv://eng.vault.azure.net/db?query=.user".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    let calls = std::fs::read_to_string(fake_bin.path().join("calls")).unwrap();
    assert_eq!(calls.matches('x').count(), 1);
    assert_eq!(
        result.get("azurekv://eng/db?query=.password").unwrap(),
        "s3cret"
    );
    assert_eq!(
        result
            .get("azurekv://eng.vault.azure.net/db?query=.user")
            .unwrap(),
        "app"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_fail_closed_on_missing() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "az", "echo missing >&2; exit 1");
    let mut p = AzureKv::new();
    p.add("azurekv://eng/db".to_string()).unwrap();
    let err = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("missing"), "{err}");
}
