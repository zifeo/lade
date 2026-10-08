use crate::family::{Family, is_raw_secret};

use super::resolve::split_scheme;
use super::secret::{DEFAULT_VAULT_TTL_MS, LadeRule, RuleTtl};

pub fn is_shell_uri(uri: &str) -> bool {
    matches!(split_scheme(uri), Some("sh" | "bash" | "zsh" | "fish"))
}

/// Body TTL used on `Put`, if this body may cache anything.
pub fn body_put_ttl_ms(rule: &LadeRule) -> Option<u32> {
    match rule.ttl() {
        Some(RuleTtl::Off) => None,
        Some(ttl) => ttl.ttl_ms(),
        None => Some(DEFAULT_VAULT_TTL_MS),
    }
}

/// Vault / file / sops / age default on. Shell needs `.ttl:`. Raw, tunnel,
/// and package never.
pub fn uri_is_cacheable(uri: &str, ttl: Option<&RuleTtl>) -> bool {
    if is_raw_secret(uri) || Family::of_uri(uri) != Family::Secret {
        return false;
    }
    match ttl {
        Some(RuleTtl::Off) => false,
        Some(RuleTtl::Window(_)) => true,
        None => !is_shell_uri(uri),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::secret::{LadeRule, RuleConfig, RuleTtl};
    use crate::window::parse_window;

    fn rule_with_ttl(ttl: Option<RuleTtl>) -> LadeRule {
        LadeRule {
            config: Some(RuleConfig {
                ttl,
                ..RuleConfig::default()
            }),
            secrets: Default::default(),
        }
    }

    #[test]
    fn vault_defaults_to_five_minutes() {
        assert_eq!(body_put_ttl_ms(&rule_with_ttl(None)), Some(300_000));
        assert!(uri_is_cacheable("op://v/i/f", None));
        assert!(uri_is_cacheable("file:///tmp/x?query=.a", None));
        assert!(!uri_is_cacheable("sh://echo hi", None));
        assert!(!uri_is_cacheable("raw://x", None));
        assert!(!uri_is_cacheable("kubectl://ctx", None));
        assert!(!uri_is_cacheable("mise://aqua/jqlang/jq@1.7.1", None));
    }

    #[test]
    fn ttl_off_disables_the_body() {
        let rule = rule_with_ttl(Some(RuleTtl::Off));
        assert!(body_put_ttl_ms(&rule).is_none());
        assert!(!uri_is_cacheable("op://v/i/f", rule.ttl()));
    }

    #[test]
    fn explicit_ttl_opts_in_shell() {
        let ttl = RuleTtl::Window(parse_window("10m").unwrap());
        assert_eq!(ttl.ttl_ms(), Some(600_000));
        assert!(uri_is_cacheable("sh://echo hi", Some(&ttl)));
        assert!(uri_is_cacheable("op://v/i/f", Some(&ttl)));
        assert!(!uri_is_cacheable("raw://x", Some(&ttl)));
    }

    #[test]
    fn ttl_parse_caps_at_24h() {
        assert!(RuleTtl::parse("24h").is_ok());
        assert!(RuleTtl::parse("1d").is_ok());
        assert!(RuleTtl::parse("25h").unwrap_err().contains("24h"));
        assert!(RuleTtl::parse("2d").is_err());
    }
}
