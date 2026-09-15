/// Product families. One short token in the CLI, box, and JSON.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Family {
    Secret,
    Tunnel,
    Bin,
}

impl Family {
    pub fn token(self) -> &'static str {
        match self {
            Family::Secret => "secret",
            Family::Tunnel => "tunnel",
            Family::Bin => "bin",
        }
    }

    pub fn spoken(self) -> &'static str {
        match self {
            Family::Secret => "secret",
            Family::Tunnel => "tunnel",
            Family::Bin => "binary",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "secret" | "env" => Some(Family::Secret),
            "tunnel" | "net" | "network" | "fwd" => Some(Family::Tunnel),
            "bin" | "cli" | "tool" | "pkg" | "package" | "apm" | "skill" => Some(Family::Bin),
            _ => None,
        }
    }

    pub fn of_uri(uri: &str) -> Self {
        if crate::mise::looks_like_spec(uri) || is_package_uri(uri) {
            return Family::Bin;
        }
        if let Some((scheme, _)) = uri.split_once("://")
            && lade_sdk::network::is_network_scheme(scheme)
        {
            return Family::Tunnel;
        }
        Family::Secret
    }
}

pub fn is_package_uri(uri: &str) -> bool {
    uri.starts_with("apm://") || uri.starts_with("skills://")
}

pub const RAW_WARNING: &str = "Raw is a value you typed, not a vault secret.";

pub fn is_raw_secret(uri: &str) -> bool {
    Family::of_uri(uri) == Family::Secret && (uri.starts_with("raw://") || !uri.contains("://"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_map_to_tokens() {
        assert_eq!(Family::parse("env"), Some(Family::Secret));
        assert_eq!(Family::parse("fwd"), Some(Family::Tunnel));
        assert_eq!(Family::parse("pkg"), Some(Family::Bin));
        assert_eq!(Family::parse("apm"), Some(Family::Bin));
        assert!(Family::parse("mise").is_none());
    }

    #[test]
    fn uri_classifies_families() {
        assert_eq!(Family::of_uri("op://vault/item/field"), Family::Secret);
        assert_eq!(Family::of_uri("raw://hello"), Family::Secret);
        assert_eq!(Family::of_uri("mise://aqua/jqlang/jq@1.7.1"), Family::Bin);
        assert_eq!(
            Family::of_uri("apm://github/destructure-command-hook"),
            Family::Bin
        );
        assert_eq!(
            Family::of_uri("skills://vercel-labs/agent-skills"),
            Family::Bin
        );
        assert_eq!(Family::of_uri("kubectl://ctx"), Family::Tunnel);
        assert_eq!(Family::of_uri("file:///tmp/x?query=.a"), Family::Secret);
    }

    #[test]
    fn raw_secret_is_typed_value() {
        assert!(is_raw_secret("raw://hello"));
        assert!(is_raw_secret("typed-in-plain"));
        assert!(!is_raw_secret("op://v/i/f"));
        assert!(!is_raw_secret("file:///tmp/x?query=.a"));
        assert!(!is_raw_secret("mise://aqua/jqlang/jq@1.7.1"));
    }
}
