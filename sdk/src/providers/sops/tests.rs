use super::*;
use crate::providers::fake_cli;
use std::path::Path;
use tempfile::tempdir;

fn path_env(dir: &tempfile::TempDir) -> HashMap<String, String> {
    HashMap::from([(
        "PATH".to_string(),
        dir.path().to_string_lossy().into_owned(),
    )])
}

#[test]
fn test_add_routing() {
    let mut p = Sops::new();
    assert!(
        p.add("sops://secrets.enc.yaml?query=.password".to_string())
            .is_ok()
    );
    assert!(
        p.add("sops://secrets.enc.yaml?query=.password&plugin=age&identity=CI_AGE".to_string())
            .is_ok()
    );
    assert!(p.add("sops://secrets.enc.yaml".to_string()).is_ok());
    assert!(p.add("sops://".to_string()).is_err());
    assert!(
        p.add("sops://secrets.enc.yaml?plugin=age".to_string())
            .is_ok()
    );
    assert!(p.add("age://blob".to_string()).is_err());
    assert!(
        parse_sops("sops://secrets.enc.yaml?plugin=age")
            .unwrap_err()
            .to_string()
            .contains("identity=")
    );
    assert!(
        parse_sops("sops://secrets.enc.yaml?plugin=age-plugin-yubikey&identity=Y")
            .unwrap_err()
            .to_string()
            .contains("plugin=yubikey")
    );
    assert_eq!(p.batch_unit(), "(path, plugin, identity)");
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_same_path_one_call() {
    let fake_bin = tempdir().unwrap();
    let counter = fake_bin.path().join("calls");
    std::fs::write(&counter, "0").unwrap();
    fake_cli(
        &fake_bin,
        "sops",
        &format!(
            r#"n=$(cat "{c}"); echo $((n + 1)) > "{c}"; echo '{{"password":"s3cret","user":"app"}}'"#,
            c = counter.display()
        ),
    );
    let mut p = Sops::new();
    p.add("sops://secrets.enc.yaml?query=.password".to_string())
        .unwrap();
    p.add("sops://secrets.enc.yaml?query=.user".to_string())
        .unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get("sops://secrets.enc.yaml?query=.password")
            .unwrap(),
        "s3cret"
    );
    assert_eq!(
        result.get("sops://secrets.enc.yaml?query=.user").unwrap(),
        "app"
    );
    assert_eq!(std::fs::read_to_string(&counter).unwrap().trim(), "1");
}

#[tokio::test]
#[cfg(unix)]
async fn test_plugin_age_remaps_identity() {
    let fake_bin = tempdir().unwrap();
    fake_cli(
        &fake_bin,
        "sops",
        r#"test -n "$SOPS_AGE_KEY" || { echo missing >&2; exit 1; }
echo '{"password":"from-named"}'"#,
    );
    let mut p = Sops::new();
    p.add("sops://secrets.enc.yaml?query=.password&plugin=age&identity=CI_AGE".to_string())
        .unwrap();
    let mut extra = path_env(&fake_bin);
    extra.insert("CI_AGE".to_string(), "AGE-SECRET-KEY-1TEST".to_string());
    let result = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get("sops://secrets.enc.yaml?query=.password&plugin=age&identity=CI_AGE")
            .unwrap(),
        "from-named"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_plugin_age_missing_named_env() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "sops", "echo '{}'");
    let mut p = Sops::new();
    p.add("sops://secrets.enc.yaml?plugin=age&identity=CI_AGE".to_string())
        .unwrap();
    let err = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("CI_AGE"), "{err}");
}

#[tokio::test]
#[cfg(unix)]
async fn test_encoded_path_same_file_one_call() {
    let fake_bin = tempdir().unwrap();
    let counter = fake_bin.path().join("calls");
    std::fs::write(&counter, "0").unwrap();
    fake_cli(
        &fake_bin,
        "sops",
        &format!(
            r#"n=$(cat "{c}"); echo $((n + 1)) > "{c}"; echo '{{"password":"s3cret"}}'"#,
            c = counter.display()
        ),
    );
    let mut p = Sops::new();
    p.add("sops://./secrets.enc.yaml?query=.password".to_string())
        .unwrap();
    p.add("sops://secrets.enc.yaml?query=.password".to_string())
        .unwrap();
    p.resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert_eq!(std::fs::read_to_string(&counter).unwrap().trim(), "1");
}

#[tokio::test]
#[cfg(unix)]
async fn test_no_query_returns_full_json() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "sops", r#"echo '{"password":"s3cret"}'"#);
    let mut p = Sops::new();
    p.add("sops://secrets.enc.yaml".to_string()).unwrap();
    let result = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap();
    assert!(
        result
            .get("sops://secrets.enc.yaml")
            .unwrap()
            .contains("s3cret")
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_plugin_yubikey_missing_binary() {
    let empty = tempdir().unwrap();
    let mut p = Sops::new();
    p.add("sops://secrets.enc.yaml?plugin=yubikey&identity=YUBI_ID".to_string())
        .unwrap();
    let mut extra = path_env(&empty);
    extra.insert("YUBI_ID".to_string(), "AGE-PLUGIN-YUBIKEY-1".to_string());
    let err = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("age-plugin-yubikey"), "{err}");
}

#[tokio::test]
#[cfg(unix)]
async fn test_aws_kms_remaps_profile() {
    let fake_bin = tempdir().unwrap();
    fake_cli(
        &fake_bin,
        "sops",
        r#"test "$AWS_PROFILE" = deploy || { echo bad >&2; exit 1; }
echo '{"password":"kms"}'"#,
    );
    let mut p = Sops::new();
    p.add(
        "sops://secrets.enc.yaml?query=.password&plugin=aws_kms&profile=DEPLOY_PROFILE".to_string(),
    )
    .unwrap();
    let mut extra = path_env(&fake_bin);
    extra.insert("DEPLOY_PROFILE".to_string(), "deploy".to_string());
    let result = p
        .resolve(Path::new("."), &extra, &Warnings::default())
        .await
        .unwrap();
    assert_eq!(
        result
            .get("sops://secrets.enc.yaml?query=.password&plugin=aws_kms&profile=DEPLOY_PROFILE")
            .unwrap(),
        "kms"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn test_resolve_cli_error() {
    let fake_bin = tempdir().unwrap();
    fake_cli(&fake_bin, "sops", "echo denied >&2; exit 1");
    let mut p = Sops::new();
    p.add("sops://secrets.enc.yaml?query=.password".to_string())
        .unwrap();
    let err = p
        .resolve(Path::new("."), &path_env(&fake_bin), &Warnings::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("SOPS error"), "{err}");
}
