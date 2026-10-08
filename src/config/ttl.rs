use crate::family::{Family, is_raw_secret};

use super::resolve::split_scheme;
use super::secret::{DEFAULT_VAULT_TTL_MS, LadeRule, RuleTtl};

pub fn is_shell_uri(uri: &str) -> bool {
    matches!(split_scheme(uri), Some("sh" | "bash" | "zsh" | "fish"))
}

/// Body TTL used on `Put`, if this body may cache anything.
pub fn body_put_ttl_ms(rule: &LadeRule, session: Option<&RuleTtl>) -> Option<u32> {
    put_ttl_ms(rule.ttl(), session)
}

/// Vault / file / sops / age default on. Shell needs `.ttl:` or a session
/// window. Raw, tunnel, and package never.
pub fn uri_is_cacheable(uri: &str, ttl: Option<&RuleTtl>, session: Option<&RuleTtl>) -> bool {
    cacheable_uri(uri, ttl, session)
}

fn put_ttl_ms(ttl: Option<&RuleTtl>, session: Option<&RuleTtl>) -> Option<u32> {
    if matches!(ttl, Some(RuleTtl::Off)) {
        return None;
    }
    match session {
        Some(RuleTtl::Off) => None,
        Some(window) => window.ttl_ms(),
        None => match ttl {
            Some(body) => body.ttl_ms(),
            None => Some(DEFAULT_VAULT_TTL_MS),
        },
    }
}

fn cacheable_uri(uri: &str, ttl: Option<&RuleTtl>, session: Option<&RuleTtl>) -> bool {
    if is_raw_secret(uri) || Family::of_uri(uri) != Family::Secret {
        return false;
    }
    if matches!(ttl, Some(RuleTtl::Off)) || matches!(session, Some(RuleTtl::Off)) {
        return false;
    }
    if matches!(session, Some(RuleTtl::Window(_))) {
        return true;
    }
    match ttl {
        Some(RuleTtl::Window(_)) => true,
        Some(RuleTtl::Off) => false,
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

    fn window(raw: &str) -> RuleTtl {
        RuleTtl::Window(parse_window(raw).unwrap())
    }

    #[test]
    fn vault_defaults_to_five_minutes() {
        assert_eq!(body_put_ttl_ms(&rule_with_ttl(None), None), Some(300_000));
        assert!(uri_is_cacheable("op://v/i/f", None, None));
        assert!(uri_is_cacheable("file:///tmp/x?query=.a", None, None));
        assert!(!uri_is_cacheable("sh://echo hi", None, None));
        assert!(!uri_is_cacheable("raw://x", None, None));
        assert!(!uri_is_cacheable("kubectl://ctx", None, None));
        assert!(!uri_is_cacheable("mise://aqua/jqlang/jq@1.7.1", None, None));
    }

    #[test]
    fn ttl_off_disables_the_body() {
        let rule = rule_with_ttl(Some(RuleTtl::Off));
        assert!(body_put_ttl_ms(&rule, None).is_none());
        assert!(!uri_is_cacheable("op://v/i/f", rule.ttl(), None));
    }

    #[test]
    fn explicit_ttl_opts_in_shell() {
        let ttl = window("10m");
        assert_eq!(ttl.ttl_ms(), Some(600_000));
        assert!(uri_is_cacheable("sh://echo hi", Some(&ttl), None));
        assert!(uri_is_cacheable("op://v/i/f", Some(&ttl), None));
        assert!(!uri_is_cacheable("raw://x", Some(&ttl), None));
    }

    #[test]
    fn ttl_parse_caps_at_24h() {
        assert!(RuleTtl::parse("24h").is_ok());
        assert!(RuleTtl::parse("1d").is_ok());
        assert!(RuleTtl::parse("25h").unwrap_err().contains("24h"));
        assert!(RuleTtl::parse("2d").is_err());
    }

    #[test]
    fn session_window_replaces_put_ttl() {
        let two_h = window("2h");
        assert_eq!(put_ttl_ms(None, Some(&two_h)), Some(7_200_000));
        assert_eq!(
            put_ttl_ms(Some(&window("10m")), Some(&two_h)),
            Some(7_200_000)
        );
    }

    #[test]
    fn session_window_opts_in_shell() {
        let two_h = window("2h");
        assert!(cacheable_uri("sh://sleep 1; echo hi", None, Some(&two_h)));
        assert!(cacheable_uri("op://v/i/f", None, Some(&two_h)));
        assert!(!cacheable_uri("raw://x", None, Some(&two_h)));
    }

    #[test]
    fn yaml_off_wins_over_session() {
        let two_h = window("2h");
        assert!(put_ttl_ms(Some(&RuleTtl::Off), Some(&two_h)).is_none());
        assert!(!cacheable_uri(
            "sh://echo hi",
            Some(&RuleTtl::Off),
            Some(&two_h)
        ));
    }

    #[test]
    fn session_off_disables_put() {
        assert!(put_ttl_ms(None, Some(&RuleTtl::Off)).is_none());
        assert!(!cacheable_uri("op://v/i/f", None, Some(&RuleTtl::Off)));
    }
}
