use super::*;
use crate::providers::{Transport, Warnings};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::thread;

fn http_env(host: &str, token: &str) -> HashMap<String, String> {
    HashMap::from([
        ("VAULT_TOKEN".to_string(), token.to_string()),
        ("LADE_VAULT_HTTP".to_string(), "1".to_string()),
        ("VAULT_ADDR".to_string(), format!("http://{host}")),
        ("PATH".to_string(), "/nonexistent".to_string()),
    ])
}

fn serve_kv(status: u16, body: &str, hits: usize) -> (String, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let body = body.to_string();
    let handle = thread::spawn(move || {
        for _ in 0..hits {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = [0u8; 8192];
            let _ = stream.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    (host, handle)
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

#[test]
fn kv_v2_url_encodes_nested_key() {
    assert_eq!(
        kv_v2_url("http://127.0.0.1:8200", "secret", "org/team"),
        "http://127.0.0.1:8200/v1/secret/data/org/team"
    );
}

#[tokio::test]
async fn test_resolve_single_field() {
    let (host, server) = serve_kv(200, r#"{"data":{"data":{"password":"s3cret"}}}"#, 1);
    let mut p = Vault::new();
    p.add(format!("vault://{host}/secret/myapp/password"))
        .unwrap();
    let result = p
        .resolve(
            Path::new("."),
            &http_env(&host, "s.token"),
            &Warnings::default(),
        )
        .await
        .unwrap();
    assert_eq!(
        result
            .get(&format!("vault://{host}/secret/myapp/password"))
            .unwrap(),
        "s3cret"
    );
    server.join().unwrap();
}

#[tokio::test]
async fn test_resolve_multiple_fields_same_key_one_call() {
    let (host, server) = serve_kv(
        200,
        r#"{"data":{"data":{"password":"s3cret","api_key":"key123"}}}"#,
        1,
    );
    let mut p = Vault::new();
    let password = format!("vault://{host}/secret/myapp/password");
    let api_key = format!("vault://{host}/secret/myapp/api_key");
    p.add(password.clone()).unwrap();
    p.add(api_key.clone()).unwrap();
    let result = p
        .resolve(
            Path::new("."),
            &http_env(&host, "s.token"),
            &Warnings::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.get(&password).unwrap(), "s3cret");
    assert_eq!(result.get(&api_key).unwrap(), "key123");
    server.join().unwrap();
}

#[tokio::test]
async fn test_resolve_missing_token() {
    let home = tempfile::tempdir().unwrap();
    let mut p = Vault::new();
    p.add("vault://localhost/secret/myapp/password".to_string())
        .unwrap();
    let extra = HashMap::from([("HOME".to_string(), home.path().display().to_string())]);
    let err = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("VAULT_TOKEN"), "{err}");
}

#[tokio::test]
async fn test_resolve_token_file() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join(".vault-token"), "file-token\n").unwrap();
    let (host, server) = serve_kv(200, r#"{"data":{"data":{"password":"from-file"}}}"#, 1);
    let mut p = Vault::new();
    p.add(format!("vault://{host}/secret/myapp/password"))
        .unwrap();
    let extra = HashMap::from([
        ("HOME".to_string(), home.path().display().to_string()),
        ("LADE_VAULT_HTTP".to_string(), "1".to_string()),
        ("PATH".to_string(), "/nonexistent".to_string()),
    ]);
    let result = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get(&format!("vault://{host}/secret/myapp/password"))
            .unwrap(),
        "from-file"
    );
    server.join().unwrap();
}

#[tokio::test]
async fn test_lade_vault_token_and_namespace() {
    let (host, server) = serve_kv(200, r#"{"data":{"data":{"password":"from-lade"}}}"#, 1);
    let mut p = Vault::new();
    p.add(format!("vault://{host}/secret/myapp/password"))
        .unwrap();
    let extra = HashMap::from([
        ("PATH".to_string(), "/nonexistent".to_string()),
        ("LADE_VAULT_TOKEN".to_string(), "lade-token".to_string()),
        ("LADE_VAULT_NAMESPACE".to_string(), "ns1".to_string()),
        ("LADE_VAULT_HTTP".to_string(), "1".to_string()),
    ]);
    let result = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get(&format!("vault://{host}/secret/myapp/password"))
            .unwrap(),
        "from-lade"
    );
    server.join().unwrap();
}

#[tokio::test]
async fn test_missing_field_names_field() {
    let (host, server) = serve_kv(200, r#"{"data":{"data":{"other":"x"}}}"#, 1);
    let mut p = Vault::new();
    p.add(format!("vault://{host}/secret/myapp/password"))
        .unwrap();
    let err = p
        .resolve(
            Path::new("."),
            &http_env(&host, "s.token"),
            &Warnings::default(),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("password"), "{err}");
    assert!(!err.to_string().contains("myapp"), "{err}");
    server.join().unwrap();
}

#[tokio::test]
async fn test_resolve_http_error() {
    let (host, server) = serve_kv(403, "permission denied", 1);
    let mut p = Vault::new();
    p.add(format!("vault://{host}/secret/myapp/password"))
        .unwrap();
    let err = p
        .resolve(
            Path::new("."),
            &http_env(&host, "s.token"),
            &Warnings::default(),
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Vault error"), "{err}");
    server.join().unwrap();
}
