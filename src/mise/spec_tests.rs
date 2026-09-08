use super::*;

#[test]
fn parse_mise_uri_aqua() {
    let spec = parse("mise://aqua/opentofu/opentofu@1.8.2").unwrap();
    assert_eq!(spec.prefix, "aqua");
    assert_eq!(spec.package, "opentofu/opentofu");
    assert_eq!(spec.version, "1.8.2");
    assert_eq!(spec.short_name(), "opentofu");
    assert_eq!(spec.backend_id(), "aqua:opentofu/opentofu");
    assert_eq!(spec.backend_slug(), "aqua-opentofu-opentofu");
    assert_eq!(spec.cli_spec(), "aqua:opentofu/opentofu@1.8.2");
}

#[test]
fn parse_github_with_options() {
    let spec =
        parse("mise://github/oxc-project/oxc[matching=oxlint,rename_exe=oxlint]@apps_v1.69.0")
            .unwrap();
    assert_eq!(spec.package, "oxc-project/oxc");
    assert_eq!(spec.version, "apps_v1.69.0");
    assert_eq!(spec.options.get("matching").unwrap(), "oxlint");
    assert_eq!(spec.options.get("rename_exe").unwrap(), "oxlint");
    assert_eq!(
        spec.cli_spec(),
        "github:oxc-project/oxc[matching=oxlint,rename_exe=oxlint]@apps_v1.69.0"
    );
}

#[test]
fn parse_http_url_with_at_in_opts() {
    let spec =
        parse("mise://http/my-tool[url=https://example.com/my-tool-v1.0.0.tar.gz]@1.0.0").unwrap();
    assert_eq!(spec.package, "my-tool");
    assert_eq!(spec.version, "1.0.0");
    assert_eq!(
        spec.options.get("url").unwrap(),
        "https://example.com/my-tool-v1.0.0.tar.gz"
    );
}

#[test]
fn parse_npm_scoped() {
    let spec = parse("mise://npm/@biomejs/biome@1.9.0").unwrap();
    assert_eq!(spec.package, "@biomejs/biome");
    assert_eq!(spec.version, "1.9.0");
    assert_eq!(spec.short_name(), "biome");
}

#[test]
fn parse_go_module() {
    let spec =
        parse("mise://go/github.com/golangci/golangci-lint/cmd/golangci-lint@1.64.0").unwrap();
    assert_eq!(spec.short_name(), "golangci-lint");
    assert_eq!(spec.version, "1.64.0");
}

#[test]
fn parse_core_rust() {
    let spec = parse("mise://core/rust@1.96.0").unwrap();
    assert_eq!(spec.package, "rust");
    assert_eq!(spec.short_name(), "rust");
    assert_eq!(spec.cli_spec(), "core:rust@1.96.0");
}

#[test]
fn reject_legacy_cli_and_other_uris() {
    let err = parse("core:rust@1.96.0").unwrap_err();
    assert!(err.contains("mise://core/rust@1.96.0"), "{err}");
    assert!(parse("op://vault/item").is_err());
    assert!(parse("mise://aqua/opentofu/opentofu").is_err());
    assert!(looks_like_spec("core:rust@1.96.0"));
    assert!(looks_like_spec("mise://aqua/jqlang/jq@1.7.1"));
    assert!(looks_like_spec("mise://http/my-tool[url=https://x]@1.0.0"));
    assert!(!looks_like_spec("op://vault/item"));
    assert!(!looks_like_spec("1.7.1"));
    assert!(!looks_like_spec("https://example.com/x"));
    assert!(!looks_like_spec("mise://tools"));
    assert!(
        parse("mise://nosuch/pkg@1.0")
            .unwrap_err()
            .contains("unknown")
    );
    assert!(parse("mise://aqua/").unwrap_err().contains("package"));
    assert!(parse("mise://core/node@24.16.0").is_ok());
}

#[test]
fn bare_version_detection() {
    assert!(looks_like_bare_version("1.7.1"));
    assert!(looks_like_bare_version("v1.64.0"));
    assert!(looks_like_bare_version("20"));
    assert!(looks_like_bare_version("latest"));
    assert!(looks_like_bare_version("1.8.2-rc1"));
    assert!(!looks_like_bare_version("hello"));
    assert!(!looks_like_bare_version("mise://aqua/jq@1.7.1"));
    assert!(!looks_like_bare_version("op://v/i/f"));
}

#[test]
fn argv0_uses_basename() {
    assert_eq!(argv0("tofu plan"), "tofu");
    assert_eq!(argv0("/usr/bin/tofu plan"), "tofu");
    assert_eq!(argv0("mise ls"), "mise");
    assert!(is_mise_argv0("mise"));
    assert!(!is_mise_argv0("tofu"));
}
