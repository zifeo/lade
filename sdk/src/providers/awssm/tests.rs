use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct RecordingApi {
    calls: AtomicUsize,
    values: HashMap<String, String>,
    aliases: HashMap<String, String>,
}

#[async_trait]
impl AwsSmApi for RecordingApi {
    async fn batch_get(&self, _: &str, ids: &[String]) -> Result<HashMap<String, String>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let mut out = HashMap::new();
        for id in ids {
            let key = self.aliases.get(id).unwrap_or(id);
            let value = self
                .values
                .get(key)
                .ok_or_else(|| anyhow!("missing {id}"))?;
            out.insert(id.clone(), value.clone());
            out.insert(key.clone(), value.clone());
        }
        Ok(out)
    }

    async fn get(&self, _: &str, id: &str, _: Option<&str>, _: Option<&str>) -> Result<String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.values
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("missing {id}"))
    }
}

#[tokio::test]
async fn test_batch_and_query() {
    let api = Arc::new(RecordingApi {
        calls: AtomicUsize::new(0),
        values: HashMap::from([(
            "myapp/db".to_string(),
            r#"{"password":"s3cret","user":"app"}"#.to_string(),
        )]),
        aliases: HashMap::new(),
    });
    let mut p = AwsSm::with_api(api.clone());
    p.add("awssm://us-east-1/myapp/db?query=.password".to_string())
        .unwrap();
    p.add("awssm://us-east-1/myapp/db?query=.user".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &HashMap::new(), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(api.calls.load(Ordering::SeqCst), 1);
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
}

#[tokio::test]
async fn test_chunk_over_batch_size() {
    let mut values = HashMap::new();
    for i in 0..21 {
        values.insert(format!("s{i}"), format!("v{i}"));
    }
    let api = Arc::new(RecordingApi {
        calls: AtomicUsize::new(0),
        values,
        aliases: HashMap::new(),
    });
    let mut p = AwsSm::with_api(api.clone());
    for i in 0..21 {
        p.add(format!("awssm://us-east-1/s{i}")).unwrap();
    }
    p.resolve(Path::new("."), &HashMap::new(), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(api.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_version_uses_get() {
    let api = Arc::new(RecordingApi {
        calls: AtomicUsize::new(0),
        values: HashMap::from([("db".to_string(), "pinned".to_string())]),
        aliases: HashMap::new(),
    });
    let mut p = AwsSm::with_api(api.clone());
    p.add("awssm://us-east-1/db?version=abc".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &HashMap::new(), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(api.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        result.get("awssm://us-east-1/db?version=abc").unwrap(),
        "pinned"
    );
}

#[tokio::test]
async fn test_arn_alias_lookup() {
    let api = Arc::new(RecordingApi {
        calls: AtomicUsize::new(0),
        values: HashMap::from([("db".to_string(), "from-arn".to_string())]),
        aliases: HashMap::from([(
            "arn:aws:secretsmanager:us-east-1:1:secret:db".to_string(),
            "db".to_string(),
        )]),
    });
    let mut p = AwsSm::with_api(api);
    p.add("awssm://us-east-1/arn:aws:secretsmanager:us-east-1:1:secret:db".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &HashMap::new(), &Warnings::default())
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
async fn test_fail_closed_on_missing() {
    let api = Arc::new(RecordingApi {
        calls: AtomicUsize::new(0),
        values: HashMap::new(),
        aliases: HashMap::new(),
    });
    let mut p = AwsSm::with_api(api);
    p.add("awssm://us-east-1/missing".to_string()).unwrap();
    let err = p
        .resolve(Path::new("."), &HashMap::new(), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("missing"), "{err}");
}

#[test]
fn test_add_routing() {
    let mut p = AwsSm::new();
    assert!(p.add("awssm://eu-west-1/name".to_string()).is_ok());
    assert!(p.add("awssm:///name".to_string()).is_err());
    assert!(p.add("vault://h/m/k/f".to_string()).is_err());
    assert_eq!(p.transport(), Transport::Sdk);
}
