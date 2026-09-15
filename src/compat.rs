use rustc_hash::FxHashSet;

use lade_sdk::compat::{self, is_secret_scheme};
use lade_sdk::network::is_network_scheme;

pub fn known_schemes<'a>(uris: impl Iterator<Item = &'a str>) -> Vec<String> {
    uris.filter_map(|uri| uri.split_once("://").map(|(scheme, _)| scheme))
        .filter(|scheme| is_secret_scheme(scheme) || is_network_scheme(scheme))
        .map(|scheme| scheme.to_string())
        .collect::<FxHashSet<_>>()
        .into_iter()
        .collect()
}

pub fn all_supported_schemes() -> Vec<String> {
    compat::all_supported_schemes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_schemes_filters_and_dedupes() {
        let uris = [
            "op://my.1password.com/v/i/f",
            "op://my.1password.com/v/i/g",
            "vault://localhost/secret/app/pass",
            "plainvalue",
            "unknown://host/path",
        ];
        let mut schemes = known_schemes(uris.iter().copied());
        schemes.sort();
        assert_eq!(schemes, vec!["op".to_string(), "vault".to_string()]);
    }

    #[test]
    fn test_known_schemes_empty() {
        assert!(known_schemes(["plain".to_string()].iter().map(|s| s.as_str())).is_empty());
    }

    #[test]
    fn test_all_supported_schemes() {
        let schemes = all_supported_schemes();
        assert!(schemes.contains(&"op".to_string()));
        assert!(schemes.contains(&"vault".to_string()));
        assert!(schemes.contains(&"awssm".to_string()));
        assert!(schemes.contains(&"kubectl".to_string()));
    }

    #[test]
    fn test_known_schemes_includes_network() {
        let uris = ["kubectl://127.0.0.1:6443/ctx/ns/pod/name/80"];
        let schemes = known_schemes(uris.into_iter());
        assert_eq!(schemes, vec!["kubectl".to_string()]);
    }
}
