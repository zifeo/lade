use semver::{Version, VersionReq};

/// Floor for PATH mise. Setup may fetch a newer official release.
pub const MISE_RANGE: &str = ">=2024.0.0";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MiseStatus {
    pub version: Option<String>,
    pub in_range: bool,
}

pub fn required() -> VersionReq {
    VersionReq::parse(MISE_RANGE).expect("MISE_RANGE is valid")
}

pub fn parse_version(raw: &str) -> Option<Version> {
    let trimmed = raw.trim();
    let rest = trimmed
        .strip_prefix("mise ")
        .or_else(|| trimmed.strip_prefix("mise"))
        .unwrap_or(trimmed)
        .trim();
    let token = rest.split_whitespace().next().unwrap_or(rest);
    let token = token.trim_start_matches('v');
    let normalized = normalize_mise(token);
    Version::parse(&normalized).ok()
}

fn normalize_mise(token: &str) -> String {
    let parts: Vec<&str> = token.split('.').collect();
    match parts.as_slice() {
        [a] => format!("{a}.0.0"),
        [a, b] => format!("{a}.{b}.0"),
        [a, b, c, ..] => {
            let c = c
                .chars()
                .take_while(|ch| ch.is_ascii_digit())
                .collect::<String>();
            format!("{a}.{b}.{}", if c.is_empty() { "0" } else { &c })
        }
        _ => token.to_string(),
    }
}

pub fn in_range(raw: &str) -> bool {
    parse_version(raw).is_some_and(|v| required().matches(&v))
}

pub fn status_from_stdout(stdout: &str) -> MiseStatus {
    let version = parse_version(stdout).map(|v| v.to_string());
    MiseStatus {
        in_range: in_range(stdout),
        version,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_calver() {
        assert!(in_range("mise 2024.8.12"));
        assert!(in_range("2025.1.0"));
        assert!(!in_range("2023.12.0"));
        assert!(!in_range("not-a-version"));
    }
}
