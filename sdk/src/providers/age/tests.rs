use super::*;
use crate::providers::fake_cli;
use ::age::armor::{ArmoredWriter, Format};
use ::age::secrecy::ExposeSecret;
use std::io::Write;
use tempfile::tempdir;

fn encrypt_armor(recipient: &::age::x25519::Recipient, plain: &str) -> String {
    let encryptor =
        ::age::Encryptor::with_recipients(std::iter::once(recipient as &dyn ::age::Recipient))
            .expect("recipient");
    let mut encrypted = Vec::new();
    let writer = encryptor
        .wrap_output(ArmoredWriter::wrap_output(&mut encrypted, Format::AsciiArmor).unwrap())
        .unwrap();
    let mut writer = writer;
    writer.write_all(plain.as_bytes()).unwrap();
    writer.finish().unwrap().finish().unwrap();
    String::from_utf8(encrypted).unwrap()
}

fn encrypt_raw(recipient: &::age::x25519::Recipient, plain: &str) -> String {
    let encryptor =
        ::age::Encryptor::with_recipients(std::iter::once(recipient as &dyn ::age::Recipient))
            .expect("recipient");
    let mut encrypted = Vec::new();
    {
        let mut writer = encryptor.wrap_output(&mut encrypted).unwrap();
        writer.write_all(plain.as_bytes()).unwrap();
        writer.finish().unwrap();
    }
    urlencoding::encode_binary(&encrypted).into_owned()
}

#[test]
fn test_parse_plugin_requires_identity() {
    let err = parse_age("age://-----BEGIN AGE ENCRYPTED FILE-----?plugin=yubikey")
        .unwrap_err()
        .to_string();
    assert!(err.contains("identity="), "{err}");
    assert!(
        parse_age("age://-----BEGIN AGE ENCRYPTED FILE-----?plugin=yubikey&identity=YUBI_ID")
            .is_ok()
    );
    assert!(parse_age("age://-----BEGIN AGE ENCRYPTED FILE-----?identity=CI_AGE").is_ok());
    assert!(parse_age("age://?plugin=yubikey&identity=YUBI_ID").is_err());
    assert!(parse_age("age://CIPHER?plugin=age-plugin-yubikey&identity=YUBI_ID").is_err());
}

#[tokio::test]
async fn test_round_trip_named_identity() {
    let identity = ::age::x25519::Identity::generate();
    let blob = encrypt_armor(&identity.to_public(), "hello");
    let uri = format!("age://{blob}?identity=CI_AGE");
    let mut p = Age::new();
    p.add(uri.clone()).unwrap();
    let extra = HashMap::from([(
        "CI_AGE".to_string(),
        identity.to_string().expose_secret().to_string(),
    )]);
    let result = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap();
    assert_eq!(result.get(&uri).unwrap(), "hello");
}

#[tokio::test]
async fn test_round_trip_identity_file() {
    let identity = ::age::x25519::Identity::generate();
    let blob = encrypt_armor(&identity.to_public(), "from-file");
    let dir = tempdir().unwrap();
    let key = dir.path().join("age.txt");
    std::fs::write(&key, identity.to_string().expose_secret()).unwrap();
    let uri = format!("age://{blob}?identity_file=CI_AGE_FILE");
    let mut p = Age::new();
    p.add(uri.clone()).unwrap();
    let extra = HashMap::from([(
        "CI_AGE_FILE".to_string(),
        key.to_string_lossy().into_owned(),
    )]);
    let result = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap();
    assert_eq!(result.get(&uri).unwrap(), "from-file");
}

#[tokio::test]
async fn test_round_trip_binary_and_percent_encoding() {
    let identity = ::age::x25519::Identity::generate();
    let blob = encrypt_raw(&identity.to_public(), "raw-hello");
    let uri = format!("age://{blob}");
    let mut p = Age::new();
    p.add(uri.clone()).unwrap();
    let extra = HashMap::from([(
        "LADE_AGE_KEY".to_string(),
        identity.to_string().expose_secret().to_string(),
    )]);
    let result = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap();
    assert_eq!(result.get(&uri).unwrap(), "raw-hello");
}

#[tokio::test]
async fn test_round_trip_and_dedupe() {
    let identity = ::age::x25519::Identity::generate();
    let blob = encrypt_armor(&identity.to_public(), "hello");
    let uri = format!("age://{blob}");
    let mut p = Age::new();
    p.add(uri.clone()).unwrap();
    p.add(uri.clone()).unwrap();
    let extra = HashMap::from([(
        "LADE_AGE_KEY".to_string(),
        identity.to_string().expose_secret().to_string(),
    )]);
    let result = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap();
    assert_eq!(result.get(&uri).unwrap(), "hello");
}

#[tokio::test]
async fn test_wrong_identity_fails() {
    let a = ::age::x25519::Identity::generate();
    let b = ::age::x25519::Identity::generate();
    let blob = encrypt_armor(&a.to_public(), "hello");
    let uri = format!("age://{blob}");
    let mut p = Age::new();
    p.add(uri).unwrap();
    let extra = HashMap::from([(
        "LADE_AGE_KEY".to_string(),
        b.to_string().expose_secret().to_string(),
    )]);
    let err = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("age decrypt failed"), "{err}");
}

#[tokio::test]
async fn test_missing_identity() {
    let mut p = Age::new();
    p.add("age://YWdl".to_string()).unwrap();
    let err = p
        .resolve(Path::new("."), &HashMap::new(), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("LADE_AGE_KEY"), "{err}");
}

#[tokio::test]
#[cfg(unix)]
async fn test_plugin_missing_binary() {
    let empty = tempdir().unwrap();
    let extra = HashMap::from([
        (
            "PATH".to_string(),
            empty.path().to_string_lossy().into_owned(),
        ),
        ("YUBI_ID".to_string(), "AGE-PLUGIN-YUBIKEY-1".to_string()),
    ]);
    let mut p = Age::new();
    p.add("age://-----BEGIN AGE ENCRYPTED FILE-----?plugin=yubikey&identity=YUBI_ID".to_string())
        .unwrap();
    let err = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("age-plugin-yubikey"), "{err}");
}

#[tokio::test]
#[cfg(unix)]
async fn test_plugin_named_identity_uses_var() {
    let dir = tempdir().unwrap();
    fake_cli(&dir, "age-plugin-yubikey", "exit 0");
    let identity = ::age::x25519::Identity::generate();
    let blob = encrypt_armor(&identity.to_public(), "hello");
    let uri = format!("age://{blob}?plugin=yubikey&identity=YUBI_ID");
    let mut p = Age::new();
    p.add(uri.clone()).unwrap();
    let extra = HashMap::from([
        (
            "PATH".to_string(),
            dir.path().to_string_lossy().into_owned(),
        ),
        (
            "YUBI_ID".to_string(),
            identity.to_string().expose_secret().to_string(),
        ),
    ]);
    let result = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap();
    assert_eq!(result.get(&uri).unwrap(), "hello");
}

#[test]
fn test_add_rejects_empty() {
    let mut p = Age::new();
    assert!(p.add("age://".to_string()).is_err());
    assert!(p.add("age://?identity=CI_AGE".to_string()).is_err());
    assert!(p.add("vault://host/mount/key/field".to_string()).is_err());
}
