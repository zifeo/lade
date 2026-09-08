use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct RecordingApi {
    calls: AtomicUsize,
    values: HashMap<(String, String, Option<String>), String>,
}

#[async_trait]
impl GcpSmApi for RecordingApi {
    async fn access(&self, project: &str, name: &str, location: Option<&str>) -> Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.values
            .get(&(
                project.to_string(),
                name.to_string(),
                location.map(str::to_string),
            ))
            .cloned()
            .ok_or_else(|| anyhow!("missing"))
    }
}

#[tokio::test]
async fn test_parallel_access() {
    let api = Arc::new(RecordingApi {
        calls: AtomicUsize::new(0),
        values: HashMap::from([
            (("p".into(), "db".into(), None), "one".into()),
            (("p".into(), "api".into(), None), "two".into()),
        ]),
    });
    let mut p = GcpSm::with_api(api.clone());
    p.add("gcpsm://p/db".to_string()).unwrap();
    p.add("gcpsm://p/api".to_string()).unwrap();
    let result = p
        .resolve(Path::new("."), &HashMap::new(), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(api.calls.load(Ordering::SeqCst), 2);
    assert_eq!(result.get("gcpsm://p/db").unwrap(), "one");
}

#[tokio::test]
async fn test_dedupe_query_and_location() {
    let api = Arc::new(RecordingApi {
        calls: AtomicUsize::new(0),
        values: HashMap::from([
            (
                ("p".into(), "db".into(), None),
                r#"{"password":"s3cret","user":"app"}"#.into(),
            ),
            (
                ("p".into(), "db".into(), Some("europe-west1".into())),
                "eu".into(),
            ),
        ]),
    });
    let mut p = GcpSm::with_api(api.clone());
    p.add("gcpsm://p/db?query=.password".to_string()).unwrap();
    p.add("gcpsm://p/db?query=.user".to_string()).unwrap();
    p.add("gcpsm://p/db?location=europe-west1".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &HashMap::new(), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(api.calls.load(Ordering::SeqCst), 2);
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
async fn test_fail_closed_on_missing() {
    let api = Arc::new(RecordingApi {
        calls: AtomicUsize::new(0),
        values: HashMap::new(),
    });
    let mut p = GcpSm::with_api(api);
    p.add("gcpsm://p/db".to_string()).unwrap();
    let err = p
        .resolve(Path::new("."), &HashMap::new(), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("missing"), "{err}");
}

#[test]
fn test_add_routing() {
    let mut p = GcpSm::new();
    assert!(p.add("gcpsm://proj/db".to_string()).is_ok());
    assert!(
        p.add("gcpsm://proj/db?location=europe-west1".to_string())
            .is_ok()
    );
    assert!(p.add("vault://h/m/k/f".to_string()).is_err());
}
