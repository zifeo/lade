use super::*;
use crate::providers::network::NetworkProviders;
use semver::Version;

#[test]
fn test_specs_have_valid_min_versions() {
    for spec in CLI_SPECS {
        assert!(
            Version::parse(spec.min_version).is_ok(),
            "{} has invalid min_version {}",
            spec.scheme,
            spec.min_version
        );
        if let Some(max) = spec.max_version {
            assert!(
                Version::parse(max).is_ok(),
                "{} has invalid max_version {max}",
                spec.scheme
            );
        }
    }
}

#[test]
fn test_all_supported_schemes_includes_secret_and_network() {
    let schemes = all_supported_schemes();
    assert!(schemes.contains(&"op".to_string()));
    assert!(schemes.contains(&"vault".to_string()));
    assert!(schemes.contains(&"awssm".to_string()));
    assert!(schemes.contains(&"azurekv".to_string()));
    assert!(schemes.contains(&"bw".to_string()));
    assert!(schemes.contains(&"age".to_string()));
    assert!(schemes.contains(&"sops".to_string()));
    assert!(schemes.contains(&"kubectl".to_string()));
    assert!(schemes.contains(&"tsh".to_string()));
}

#[test]
fn tunnel_specs_match_network_providers() {
    let providers = NetworkProviders::new();
    for spec in CLI_SPECS.iter().filter(|spec| spec.tunnel) {
        assert!(
            providers.provider(spec.scheme).is_some(),
            "CLI_SPECS tunnel '{}' is missing from NetworkProviders",
            spec.scheme
        );
        assert!(is_network_scheme(spec.scheme));
    }
    for scheme in ["kubectl", "kubefwd", "tsh", "ssh"] {
        assert!(
            is_network_scheme(scheme),
            "{scheme} must be a tunnel scheme"
        );
    }
    assert!(!is_network_scheme("op"));
    assert!(!is_network_scheme("file"));
}

#[test]
fn package_clis_live_in_the_spec_table() {
    let apm = spec_for("apm").expect("apm");
    assert_eq!(apm.bin, "apm");
    assert_eq!(apm.mise, Some("github/microsoft/apm"));
    let skills = spec_for("skills").expect("skills");
    assert_eq!(skills.bin, "skills");
    assert_eq!(skills.mise, Some("npm/skills"));
    assert_eq!(
        spec_for_bin("op").and_then(|spec| spec.docs),
        Some("https://developer.1password.com/docs/cli/get-started/")
    );
}
