use super::*;
use crate::providers::Warnings;
use httpmock::prelude::*;
use std::path::Path;

fn token_env() -> HashMap<String, String> {
    HashMap::from([
        ("VAULT_TOKEN".to_string(), "s.token".to_string()),
        ("LADE_VAULT_HTTP".to_string(), "1".to_string()),
    ])
}

#[test]
fn test_add_routing() {
    let mut p = Vault::new();
    assert!(
        p.add("vault://localhost/secret/myapp/password".to_string())
            .is_ok()
    );
    assert!(p.add("doppler://host/proj/env/VAR".to_string()).is_err());
    assert_eq!(p.transport(), Transport::Sdk);
    assert_eq!(p.batch_unit(), "(host, mount, key)");
}

#[tokio::test]
async fn test_resolve_single_field() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/secret/data/myapp")
            .header("X-Vault-Token", "s.token");
        then.status(200)
            .json_body(serde_json::json!({"data":{"data":{"password":"s3cret"}}}));
    });
    let mut p = Vault::new();
    p.add(format!(
        "vault://{}/secret/myapp/password",
        server.address()
    ))
    .unwrap();
    let result = p
        .resolve(Path::new("."), &token_env(), &Warnings::default())
        .await
        .unwrap();
    mock.assert();
    assert_eq!(
        result
            .get(&format!(
                "vault://{}/secret/myapp/password",
                server.address()
            ))
            .unwrap(),
        "s3cret"
    );
}

#[tokio::test]
async fn test_resolve_multiple_fields_same_key_one_call() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET).path("/v1/secret/data/myapp");
        then.status(200).json_body(serde_json::json!({
            "data":{"data":{"password":"s3cret","api_key":"key123"}}
        }));
    });
    let mut p = Vault::new();
    let host = server.address().to_string();
    p.add(format!("vault://{host}/secret/myapp/password"))
        .unwrap();
    p.add(format!("vault://{host}/secret/myapp/api_key"))
        .unwrap();
    let result = p
        .resolve(Path::new("."), &token_env(), &Warnings::default())
        .await
        .unwrap();
    mock.assert_hits(1);
    assert_eq!(
        result
            .get(&format!("vault://{host}/secret/myapp/password"))
            .unwrap(),
        "s3cret"
    );
    assert_eq!(
        result
            .get(&format!("vault://{host}/secret/myapp/api_key"))
            .unwrap(),
        "key123"
    );
}

#[tokio::test]
async fn test_resolve_missing_token() {
    let mut p = Vault::new();
    p.add("vault://localhost/secret/myapp/password".to_string())
        .unwrap();
    let err = p
        .resolve(Path::new("."), &HashMap::new(), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("VAULT_TOKEN"), "{err}");
}

#[tokio::test]
async fn test_lade_vault_token_and_namespace() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/v1/secret/data/myapp")
            .header("X-Vault-Token", "lade-token")
            .header("X-Vault-Namespace", "ns1");
        then.status(200)
            .json_body(serde_json::json!({"data":{"data":{"password":"from-lade"}}}));
    });
    let mut p = Vault::new();
    p.add(format!(
        "vault://{}/secret/myapp/password",
        server.address()
    ))
    .unwrap();
    let extra = HashMap::from([
        ("LADE_VAULT_TOKEN".to_string(), "lade-token".to_string()),
        ("LADE_VAULT_NAMESPACE".to_string(), "ns1".to_string()),
        ("LADE_VAULT_HTTP".to_string(), "1".to_string()),
    ]);
    let result = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap();
    mock.assert();
    assert_eq!(
        result
            .get(&format!(
                "vault://{}/secret/myapp/password",
                server.address()
            ))
            .unwrap(),
        "from-lade"
    );
}

#[tokio::test]
async fn test_missing_field_names_field() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v1/secret/data/myapp");
        then.status(200)
            .json_body(serde_json::json!({"data":{"data":{"other":"x"}}}));
    });
    let mut p = Vault::new();
    p.add(format!(
        "vault://{}/secret/myapp/password",
        server.address()
    ))
    .unwrap();
    let err = p
        .resolve(Path::new("."), &token_env(), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("password"), "{err}");
    assert!(!err.to_string().contains("myapp"), "{err}");
}

#[tokio::test]
async fn test_resolve_http_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/v1/secret/data/myapp");
        then.status(403).body("permission denied");
    });
    let mut p = Vault::new();
    p.add(format!(
        "vault://{}/secret/myapp/password",
        server.address()
    ))
    .unwrap();
    let err = p
        .resolve(Path::new("."), &token_env(), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Vault error"), "{err}");
}
