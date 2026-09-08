use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct RecordingApi {
    calls: AtomicUsize,
    values: HashMap<(String, String), String>,
}

#[async_trait]
impl AzureKvApi for RecordingApi {
    async fn get(&self, vault: &str, name: &str) -> Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.values
            .get(&(vault.to_string(), name.to_string()))
            .cloned()
            .ok_or_else(|| anyhow!("missing"))
    }
}

#[tokio::test]
async fn test_parallel_gets() {
    let api = Arc::new(RecordingApi {
        calls: AtomicUsize::new(0),
        values: HashMap::from([
            (("eng.vault.azure.net".into(), "db".into()), "one".into()),
            (("eng.vault.azure.net".into(), "api".into()), "two".into()),
        ]),
    });
    let mut p = AzureKv::with_api(api.clone());
    p.add("azurekv://eng/db".to_string()).unwrap();
    p.add("azurekv://eng/api".to_string()).unwrap();
    let result = p
        .resolve(Path::new("."), &HashMap::new(), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(api.calls.load(Ordering::SeqCst), 2);
    assert_eq!(result.get("azurekv://eng/db").unwrap(), "one");
    assert_eq!(result.get("azurekv://eng/api").unwrap(), "two");
}

#[tokio::test]
async fn test_dedupe_and_query() {
    let api = Arc::new(RecordingApi {
        calls: AtomicUsize::new(0),
        values: HashMap::from([(
            ("eng.vault.azure.net".into(), "db".into()),
            r#"{"password":"s3cret","user":"app"}"#.into(),
        )]),
    });
    let mut p = AzureKv::with_api(api.clone());
    p.add("azurekv://eng/db?query=.password".to_string())
        .unwrap();
    p.add("azurekv://eng.vault.azure.net/db?query=.user".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &HashMap::new(), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(api.calls.load(Ordering::SeqCst), 1);
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
async fn test_fail_closed_on_missing() {
    let api = Arc::new(RecordingApi {
        calls: AtomicUsize::new(0),
        values: HashMap::new(),
    });
    let mut p = AzureKv::with_api(api);
    p.add("azurekv://eng/db".to_string()).unwrap();
    let err = p
        .resolve(Path::new("."), &HashMap::new(), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("missing"), "{err}");
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
    assert!(p.add("azurekv://eng.example.com/db".to_string()).is_err());
    assert!(p.add("vault://h/m/k/f".to_string()).is_err());
}
