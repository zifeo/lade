use semver::Version;

use super::error::Error;
use super::spec::{self, Spec};
use super::unlistable;

pub(super) fn concrete_version(spec: &Spec) -> Result<String, Error> {
    if spec::version_is_concrete(&spec.version) {
        return Ok(spec.version.clone());
    }
    let listed = match super::latest_matching(spec) {
        Ok(version) => Some(version),
        Err(err) => return Err(Error::install(err.to_string())),
    };
    select_concrete(spec, listed.as_deref())
}

pub(super) fn concrete_for_update(spec: &Spec, current: &str) -> Result<String, Error> {
    if spec::version_is_concrete(&spec.version) {
        return Ok(spec.version.clone());
    }
    Ok(keep_if_newer(current, &concrete_version(spec)?))
}

pub(super) fn select_concrete(spec: &Spec, listed: Option<&str>) -> Result<String, Error> {
    if spec::version_is_concrete(&spec.version) {
        return Ok(spec.version.clone());
    }
    if let Some(version) = listed
        && spec::version_is_concrete(version)
        && constraint_matches(&spec.version, version)
    {
        return Ok(version.to_string());
    }
    if let Some(version) = unlistable::version_for(&spec.backend_id())
        && spec::version_is_concrete(version)
        && constraint_matches(&spec.version, version)
    {
        return Ok(version.to_string());
    }
    Err(Error::no_listed_version(&spec.backend_id(), &spec.version))
}

pub(super) fn keep_if_newer(current: &str, next: &str) -> String {
    let prev = Version::parse(current.trim_start_matches('v'));
    let new = Version::parse(next.trim_start_matches('v'));
    match (prev, new) {
        (Ok(prev), Ok(new)) if new < prev => current.to_string(),
        _ => next.to_string(),
    }
}

pub(super) fn highest_matching(constraint: &str, versions: &[String]) -> Option<String> {
    let mut best: Option<(Version, String)> = None;
    for raw in versions {
        let version = raw.trim();
        if !spec::version_is_concrete(version) || !constraint_matches(constraint, version) {
            continue;
        }
        let Ok(parsed) = Version::parse(version.trim_start_matches('v')) else {
            continue;
        };
        if best.as_ref().is_none_or(|(prev, _)| &parsed > prev) {
            best = Some((parsed, version.to_string()));
        }
    }
    best.map(|(_, version)| version)
}

fn constraint_matches(constraint: &str, version: &str) -> bool {
    if spec::version_is_floating(constraint) {
        return spec::version_is_concrete(version);
    }
    if !spec::version_is_range(constraint) {
        return constraint == version;
    }
    let Ok(req) = semver::VersionReq::parse(constraint) else {
        return false;
    };
    let Ok(found) = Version::parse(version.trim_start_matches('v')) else {
        return false;
    };
    req.matches(&found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op_range() -> Spec {
        spec::parse("mise://aqua/1password/cli@>=2.18.0").unwrap()
    }

    #[test]
    fn empty_listing_uses_the_known_concrete_version() {
        let version = select_concrete(&op_range(), None).unwrap();
        assert_eq!(version, "2.39.0");
        assert!(spec::version_is_concrete(&version));
    }

    #[test]
    fn listed_version_wins_over_the_known_one() {
        assert_eq!(
            select_concrete(&op_range(), Some("2.40.0")).unwrap(),
            "2.40.0"
        );
    }

    #[test]
    fn listed_version_outside_the_range_is_not_used() {
        assert_eq!(
            select_concrete(&op_range(), Some("1.0.0")).unwrap(),
            "2.39.0"
        );
    }

    #[test]
    fn a_listed_range_is_not_returned() {
        assert_eq!(
            select_concrete(&op_range(), Some(">=2.18.0")).unwrap(),
            "2.39.0"
        );
    }

    #[test]
    fn unlistable_gap_is_an_error_before_install() {
        let spec = spec::parse("mise://aqua/jqlang/jq@>=1.7.0").unwrap();
        let err = select_concrete(&spec, None).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("aqua:jqlang/jq"), "{text}");
        assert!(text.contains(">=1.7.0"), "{text}");
    }

    #[test]
    fn known_op_satisfies_the_implied_range() {
        let pins = super::super::implied::pins_for(&["op://v/i/f".to_string()], &[]);
        assert_eq!(pins.len(), 1);
        let version = select_concrete(&pins[0].1, None).unwrap();
        assert!(spec::version_is_concrete(&version));
        assert_eq!(version, "2.39.0");
    }

    #[test]
    fn update_does_not_downgrade() {
        assert_eq!(keep_if_newer("2.40.0", "2.39.0"), "2.40.0");
        assert_eq!(keep_if_newer("2.31.1", "2.39.0"), "2.39.0");
    }

    #[test]
    fn ls_remote_keeps_the_highest_version_inside_the_range() {
        let versions = [
            "1.0.0",
            "17.1.4",
            "17.1.5",
            "18.10.0",
            "19.0.0-beta.1",
            ">=17.1.5",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        assert_eq!(
            highest_matching(">=17.1.5", &versions).as_deref(),
            Some("18.10.0")
        );
    }

    #[test]
    fn update_keeps_an_exact_pin() {
        let spec = spec::parse("mise://aqua/1password/cli@2.31.0").unwrap();
        assert_eq!(concrete_for_update(&spec, "2.40.0").unwrap(), "2.31.0");
    }

    #[test]
    fn every_provider_resolves_to_a_concrete_install() {
        for cli in lade_sdk::compat::CLI_SPECS {
            let Some(row) = super::super::implied::by_scheme(cli.scheme) else {
                assert_eq!(cli.scheme, "ssh");
                assert!(cli.mise.is_none(), "{}", cli.scheme);
                continue;
            };
            let parsed = spec::parse(&row.uri).unwrap_or_else(|e| panic!("{}: {e}", cli.scheme));
            let version = select_concrete(&parsed, Some(cli.min_version))
                .unwrap_or_else(|e| panic!("{}: {e}", cli.scheme));
            assert_eq!(version, cli.min_version, "{}", cli.scheme);
            assert!(spec::version_is_concrete(&version), "{}", cli.scheme);
            let arg = spec::at_version(&parsed, &version).install_arg();
            assert!(
                arg.ends_with(&format!("@{version}")),
                "{} install arg {arg}",
                cli.scheme
            );
            assert!(!arg.contains(">="), "{arg}");
            assert!(!arg.contains("latest"), "{arg}");
            match unlistable::version_for(&parsed.backend_id()) {
                Some(fallback) => {
                    let resolved = select_concrete(&parsed, None)
                        .unwrap_or_else(|e| panic!("{}: {e}", cli.scheme));
                    assert_eq!(resolved, fallback, "{}", cli.scheme);
                    assert!(
                        constraint_matches(&parsed.version, &resolved),
                        "{}",
                        cli.scheme
                    );
                }
                None => {
                    let err = select_concrete(&parsed, None).unwrap_err();
                    assert!(
                        err.to_string().contains(&parsed.backend_id()),
                        "{}: {err}",
                        cli.scheme
                    );
                }
            }
        }
    }
}
