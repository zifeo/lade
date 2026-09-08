pub fn fake_cli(dir: &tempfile::TempDir, name: &str, script_body: &str) {
    #[cfg(unix)]
    {
        // Avoid inherited writable descriptors causing ETXTBSY: https://github.com/rust-lang/rust/issues/114554
        use std::io::Write;
        use std::process::{Command, Stdio};

        let path = dir.path().join(name);
        let mut child = Command::new("/bin/sh")
            .args(["-c", "cat > \"$1\" && chmod 755 \"$1\"", "sh"])
            .arg(path)
            .stdin(Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        write!(stdin, "#!/bin/sh\n{script_body}\n").unwrap();
        drop(stdin);
        assert!(child.wait().unwrap().success());
    }
}

use super::*;

fn has_work_for(scheme: &str, uri: &str) -> bool {
    let mut p = Providers::new();
    p.add(uri.to_string()).unwrap();
    p.by_scheme
        .get(scheme)
        .map(|prov| prov.has_work())
        .unwrap_or(false)
}

fn fallback_has_work(uri: &str) -> bool {
    let mut p = Providers::new();
    p.add(uri.to_string()).unwrap();
    p.fallback.has_work()
}

#[test]
fn test_dispatch_doppler() {
    assert!(has_work_for(
        "doppler",
        "doppler://api.doppler.com/proj/env/KEY"
    ));
    assert!(!fallback_has_work("doppler://api.doppler.com/proj/env/KEY"));
}

#[test]
fn test_dispatch_vault() {
    assert!(has_work_for("vault", "vault://localhost/secret/app/pass"));
    assert!(!fallback_has_work("vault://localhost/secret/app/pass"));
}

#[test]
fn test_dispatch_op() {
    assert!(has_work_for("op", "op://my.1password.com/vault/item/field"));
    assert!(!fallback_has_work("op://my.1password.com/vault/item/field"));
}

#[test]
fn test_dispatch_plain_value_to_fallback() {
    assert!(fallback_has_work("plainvalue"));
}

#[test]
fn test_dispatch_bang_escaped_to_fallback() {
    assert!(fallback_has_work("!escaped"));
}

#[test]
fn test_dispatch_unknown_scheme_to_fallback() {
    assert!(fallback_has_work("foo://some/path"));
}

#[test]
fn test_dispatch_file_without_query_falls_back_to_raw() {
    // file:// without ?query= is rejected by File::add and must land on Raw.
    assert!(fallback_has_work("file:///path/to/config.json"));
    assert!(!has_work_for("file", "file:///path/to/config.json"));
}

#[test]
fn test_dispatch_file_with_query_goes_to_file() {
    assert!(has_work_for(
        "file",
        "file:///path/to/config.json?query=.key"
    ));
    assert!(!fallback_has_work("file:///path/to/config.json?query=.key"));
}

#[test]
fn test_dispatch_awssm() {
    assert!(has_work_for("awssm", "awssm://us-east-1/myapp/db"));
    assert!(!fallback_has_work("awssm://us-east-1/myapp/db"));
}

#[test]
fn test_dispatch_age() {
    assert!(has_work_for("age", "age://YWdlLWVuY3J5cHRpb24ub3Jn"));
    assert!(!fallback_has_work("age://YWdlLWVuY3J5cHRpb24ub3Jn"));
}

#[test]
fn test_dispatch_sops() {
    assert!(has_work_for(
        "sops",
        "sops://secrets.enc.yaml?query=.password"
    ));
    assert!(!fallback_has_work(
        "sops://secrets.enc.yaml?query=.password"
    ));
}

#[test]
fn test_dispatch_azuresm() {
    assert!(has_work_for("azuresm", "azuresm://eng/db"));
    assert!(!fallback_has_work("azuresm://eng/db"));
}

#[test]
fn test_dispatch_gcpsm() {
    assert!(has_work_for("gcpsm", "gcpsm://proj/db"));
    assert!(!fallback_has_work("gcpsm://proj/db"));
}

#[test]
fn test_empty_age_does_not_fall_back_to_raw() {
    let mut p = Providers::new();
    assert!(p.add("age://".to_string()).is_err());
    assert!(!p.fallback.has_work());
}
