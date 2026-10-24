use super::*;
use crate::providers::{Transport, Warnings};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::mpsc;
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

fn serve_kv_capture(
    status: u16,
    body: &str,
) -> (String, mpsc::Receiver<String>, thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let body = body.to_string();
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut buf = [0u8; 8192];
        let n = stream.read(&mut buf).unwrap_or(0);
        let req = String::from_utf8_lossy(&buf[..n]);
        let target = req
            .lines()
            .next()
            .unwrap_or("")
            .split_whitespace()
            .nth(1)
            .unwrap_or("")
            .to_string();
        let _ = tx.send(target);
        let resp = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let _ = stream.write_all(resp.as_bytes());
    });
    (host, rx, handle)
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

#[test]
fn path_parts_nested_key() {
    let url = Url::parse("vault://127.0.0.1:8200/secret/org/team/password").unwrap();
    let parts = path_parts(&url).unwrap();
    assert_eq!(parts.mount, "secret");
    assert_eq!(parts.key, "org/team");
    assert_eq!(parts.field, "password");
}

#[test]
fn path_parts_rejects_empty_key_or_field() {
    let missing_key = Url::parse("vault://127.0.0.1:8200/secret/password").unwrap();
    assert!(path_parts(&missing_key).is_err());
    let empty_key = Url::parse("vault://127.0.0.1:8200/secret//password").unwrap();
    assert!(path_parts(&empty_key).is_err());
    let empty_field = Url::parse("vault://127.0.0.1:8200/secret/org/team/").unwrap();
    assert!(path_parts(&empty_field).is_err());
}

#[test]
fn compose_add_uri_nested_key_round_trips() {
    let extras = HashMap::from([
        ("field".to_string(), "password".to_string()),
        ("host".to_string(), "127.0.0.1:8200".to_string()),
    ]);
    let p = Vault::new();
    let uri = p.compose_add_uri("org/team", &extras).unwrap();
    let parts = path_parts(&Url::parse(&uri).unwrap()).unwrap();
    assert_eq!(parts.mount, "secret");
    assert_eq!(parts.key, "org/team");
    assert_eq!(parts.field, "password");
    assert_eq!(uri, parts.uri("127.0.0.1:8200"));
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
async fn test_resolve_nested_key() {
    let (host, path_rx, server) =
        serve_kv_capture(200, r#"{"data":{"data":{"password":"s3cret"}}}"#);
    let mut p = Vault::new();
    let uri = format!("vault://{host}/secret/org/team/password");
    p.add(uri.clone()).unwrap();
    let result = p
        .resolve(
            Path::new("."),
            &http_env(&host, "s.token"),
            &Warnings::default(),
        )
        .await
        .unwrap();
    assert_eq!(result.get(&uri).unwrap(), "s3cret");
    assert_eq!(path_rx.recv().unwrap(), "/v1/secret/data/org/team");
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
