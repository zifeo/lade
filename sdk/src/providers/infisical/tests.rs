use super::*;
use httpmock::prelude::*;
use std::path::Path;

fn token_env() -> HashMap<String, String> {
    HashMap::from([("INFISICAL_TOKEN".to_string(), "tok".to_string())])
}

#[test]
fn test_add_routing() {
    let mut p = Infisical::new();
    assert!(
        p.add("infisical://app.infisical.com/proj123/dev/MY_SECRET".to_string())
            .is_ok()
    );
    assert!(p.add("vault://host/mount/key/field".to_string()).is_err());
    assert!(
        p.add("infisical://app.infisical.com/proj123/dev/".to_string())
            .is_err()
    );
    assert_eq!(p.transport(), Transport::Sdk);
}

#[tokio::test]
async fn test_resolve_single_var() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/api/v3/secrets/raw")
            .query_param("workspaceId", "proj123")
            .query_param("environment", "dev")
            .query_param("secretPath", "/");
        then.status(200).json_body(serde_json::json!({
            "secrets":[{"secretKey":"MY_SECRET","secretValue":"infisical_value"}]
        }));
    });
    let mut p = Infisical::new();
    let uri = format!("infisical://{}/proj123/dev/MY_SECRET", server.address());
    p.add(uri.clone()).unwrap();
    let result = p
        .resolve(Path::new("."), &token_env(), &Warnings::default())
        .await
        .unwrap();
    mock.assert();
    assert_eq!(result.get(&uri).unwrap(), "infisical_value");
}

#[tokio::test]
async fn test_resolve_same_path_one_call() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET).path("/api/v3/secrets/raw");
        then.status(200).json_body(serde_json::json!({
            "secrets":[
                {"secretKey":"A","secretValue":"one"},
                {"secretKey":"B","secretValue":"two"}
            ]
        }));
    });
    let mut p = Infisical::new();
    let host = server.address().to_string();
    let a = format!("infisical://{host}/proj123/dev/A");
    let b = format!("infisical://{host}/proj123/dev/B");
    p.add(a.clone()).unwrap();
    p.add(b.clone()).unwrap();
    let result = p
        .resolve(Path::new("."), &token_env(), &Warnings::default())
        .await
        .unwrap();
    mock.assert_hits(1);
    assert_eq!(result.get(&a).unwrap(), "one");
    assert_eq!(result.get(&b).unwrap(), "two");
}

#[tokio::test]
async fn test_resolve_percent_encoded_variable_name() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/api/v3/secrets/raw");
        then.status(200).json_body(serde_json::json!({
            "secrets":[{"key":"MY+SECRET","value":"decoded_value"}]
        }));
    });
    let mut p = Infisical::new();
    let uri = format!("infisical://{}/proj123/dev/MY%2BSECRET", server.address());
    p.add(uri.clone()).unwrap();
    let result = p
        .resolve(Path::new("."), &token_env(), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(result.get(&uri).unwrap(), "decoded_value");
}

#[tokio::test]
async fn test_resolve_missing_variable_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/api/v3/secrets/raw");
        then.status(200)
            .json_body(serde_json::json!({"secrets":[]}));
    });
    let mut p = Infisical::new();
    p.add(format!(
        "infisical://{}/proj123/dev/MY_SECRET",
        server.address()
    ))
    .unwrap();
    let result = p
        .resolve(Path::new("."), &token_env(), &Warnings::default())
        .await;
    assert!(result.unwrap_err().to_string().contains("not found"));
}

#[tokio::test]
async fn test_nested_folder_path_and_http_error() {
    let server = MockServer::start();
    let mock = server.mock(|when, then| {
        when.method(GET)
            .path("/api/v3/secrets/raw")
            .query_param("secretPath", "/My Folder");
        then.status(200).json_body(serde_json::json!({
            "secrets":[{"secretKey":"KEY","secretValue":"nested"}]
        }));
    });
    let mut p = Infisical::new();
    let uri = format!(
        "infisical://{}/proj123/dev/My%20Folder/KEY",
        server.address()
    );
    p.add(uri.clone()).unwrap();
    let result = p
        .resolve(Path::new("."), &token_env(), &Warnings::default())
        .await
        .unwrap();
    mock.assert();
    assert_eq!(result.get(&uri).unwrap(), "nested");
}

#[tokio::test]
async fn test_resolve_http_error() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/api/v3/secrets/raw");
        then.status(403).body("forbidden");
    });
    let mut p = Infisical::new();
    p.add(format!(
        "infisical://{}/proj123/dev/MY_SECRET",
        server.address()
    ))
    .unwrap();
    let err = p
        .resolve(Path::new("."), &token_env(), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Infisical error"), "{err}");
    assert!(err.to_string().contains("403"), "{err}");
}

#[tokio::test]
async fn test_resolve_missing_token() {
    let mut p = Infisical::new();
    p.add("infisical://app.infisical.com/proj123/dev/MY_SECRET".to_string())
        .unwrap();
    let err = p
        .resolve(Path::new("."), &HashMap::new(), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("INFISICAL_TOKEN"), "{err}");
}
