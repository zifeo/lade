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

#[tokio::test]
#[cfg(unix)]
async fn test_parallel_access() {
    let fake_bin = tempdir().unwrap();
    fake_cli(
        &fake_bin,
        "gcloud",
        r#"
secret=""
for a in "$@"; do
  case "$a" in
    --secret=*) secret="${a#--secret=}" ;;
  esac
done
if [ "$secret" = "db" ]; then printf '%s' one; exit 0; fi
if [ "$secret" = "api" ]; then printf '%s' two; exit 0; fi
echo missing >&2
exit 1
"#,
    );
    let mut p = GcpSm::new();
    p.add("gcpsm://p/db".to_string()).unwrap();
    p.add("gcpsm://p/api".to_string()).unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(result.get("gcpsm://p/db").unwrap(), "one");
    assert_eq!(result.get("gcpsm://p/api").unwrap(), "two");
}

#[tokio::test]
#[cfg(unix)]
async fn test_dedupe_query_and_location() {
    let fake_bin = tempdir().unwrap();
    let calls = fake_bin.path().join("calls");
    fake_cli(
        &fake_bin,
        "gcloud",
        &format!(
            r#"
echo x >> '{calls}'
loc=""
for a in "$@"; do
  case "$a" in
    --location=*) loc="${{a#--location=}}" ;;
  esac
done
if [ -n "$loc" ]; then printf '%s' eu; exit 0; fi
printf '%s' '{{"password":"s3cret","user":"app"}}'
"#,
            calls = calls.display()
        ),
    );
    let mut p = GcpSm::new();
    p.add("gcpsm://p/db?query=.password".to_string()).unwrap();
    p.add("gcpsm://p/db?query=.user".to_string()).unwrap();
    p.add("gcpsm://p/db?location=europe-west1".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    let calls = std::fs::read_to_string(fake_bin.path().join("calls")).unwrap();
    assert_eq!(calls.matches('x').count(), 2);
    assert_eq!(
        result.get("gcpsm://p/db?query=.password").unwrap(),
        "s3cret"
    );
    assert_eq!(result.get("gcpsm://p/db?query=.user").unwrap(), "app");
    assert_eq!(
        result.get("gcpsm://p/db?location=europe-west1").unwrap(),
        "eu"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_fail_closed() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "gcloud", "echo missing >&2; exit 1");
    let mut p = GcpSm::new();
    p.add("gcpsm://p/db".to_string()).unwrap();
    let err = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("missing"), "{err}");
}

#[test]
fn test_add_routing() {
    let mut p = GcpSm::new();
    assert!(p.add("gcpsm://proj/db".to_string()).is_ok());
    assert!(p.add("gcpsm://proj/".to_string()).is_err());
    assert!(p.add("vault://h/m/k/f".to_string()).is_err());
    assert_eq!(p.transport(), Transport::Cli);
}
