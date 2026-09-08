use std::collections::BTreeMap;
use std::path::Path;

pub const BACKENDS: &[&str] = &[
    "aqua", "github", "gitlab", "forgejo", "http", "s3", "core", "npm", "pipx", "cargo", "go",
    "gem", "dotnet", "spm", "conda", "pkgx", "asdf", "vfox",
];

const SCHEME: &str = "mise://";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub prefix: String,
    pub package: String,
    pub options: BTreeMap<String, String>,
    pub version: String,
    pub uri: String,
}

impl Spec {
    pub fn backend_id(&self) -> String {
        format!("{}:{}", self.prefix, self.package)
    }

    pub fn short_name(&self) -> &str {
        self.package
            .rsplit(['/', ':'])
            .next()
            .filter(|part| !part.is_empty())
            .unwrap_or(self.package.as_str())
    }

    pub fn backend_slug(&self) -> String {
        format!("{}-{}", self.prefix, self.package.replace(['/', ':'], "-"))
    }

    pub fn cli_spec(&self) -> String {
        if self.options.is_empty() {
            return format!("{}:{}@{}", self.prefix, self.package, self.version);
        }
        let opts = self
            .options
            .iter()
            .map(|(key, value)| {
                if value.is_empty() {
                    key.clone()
                } else {
                    format!("{key}={value}")
                }
            })
            .collect::<Vec<_>>()
            .join(",");
        format!("{}:{}[{opts}]@{}", self.prefix, self.package, self.version)
    }
}

pub fn looks_like_spec(value: &str) -> bool {
    is_mise_uri(value) || is_legacy_cli_spec(value)
}

pub fn looks_like_bare_version(value: &str) -> bool {
    if value.is_empty() || value.contains(':') || value.contains('/') {
        return false;
    }
    let trimmed = value.strip_prefix('v').unwrap_or(value);
    if matches!(trimmed, "latest" | "lts") {
        return true;
    }
    let num = trimmed.split_once(['-', '+']).map_or(trimmed, |(n, _)| n);
    !num.is_empty()
        && num.chars().all(|ch| ch.is_ascii_digit() || ch == '.')
        && num.chars().any(|ch| ch.is_ascii_digit())
}

pub fn parse(value: &str) -> Result<Spec, String> {
    if is_legacy_cli_spec(value) {
        return Err(format!(
            "`{value}` is the mise CLI form. In lade.yml use {}.",
            legacy_to_uri(value)
        ));
    }
    let Some(rest) = value.strip_prefix(SCHEME) else {
        return Err(format!(
            "A pin is a Lade URI: mise://<backend>/<package>@<version>. Example: mise://core/rust@1.96.0. Got {value}"
        ));
    };
    let Some(slash) = rest.find('/') else {
        return Err(format!(
            "pin '{value}' needs mise://<backend>/<package>@<version>"
        ));
    };
    let prefix = &rest[..slash];
    if !BACKENDS.contains(&prefix) {
        return Err(format!(
            "unknown mise backend '{prefix}'. Use a name from `mise backends ls`."
        ));
    }
    let after = &rest[slash + 1..];
    if after.is_empty() {
        return Err(format!("pin '{value}' is missing a package name"));
    }
    let (package, options, version) = split_package_opts_version(after)
        .map_err(|e| format!("{e}. Example: mise://aqua/owner/repo@1.2.3"))?;
    if package.is_empty() {
        return Err(format!("pin '{value}' is missing a package name"));
    }
    if version.is_empty() {
        return Err(format!(
            "pin '{value}' is missing @version. Example: mise://{prefix}/{package}@1.2.3"
        ));
    }
    Ok(Spec {
        prefix: prefix.to_string(),
        package,
        options,
        version,
        uri: value.to_string(),
    })
}

fn is_mise_uri(value: &str) -> bool {
    let Some(rest) = value.strip_prefix(SCHEME) else {
        return false;
    };
    let backend = rest.split(['/', '[']).next().unwrap_or("");
    BACKENDS.contains(&backend)
}

fn is_legacy_cli_spec(value: &str) -> bool {
    let Some((prefix, rest)) = value.split_once(':') else {
        return false;
    };
    !rest.starts_with("//") && BACKENDS.contains(&prefix)
}

fn legacy_to_uri(value: &str) -> String {
    let Some((prefix, rest)) = value.split_once(':') else {
        return format!("{SCHEME}{value}");
    };
    format!("{SCHEME}{prefix}/{rest}")
}

fn split_package_opts_version(
    rest: &str,
) -> Result<(String, BTreeMap<String, String>, String), String> {
    if let Some(opt_start) = rest.find('[') {
        let after_open = &rest[opt_start + 1..];
        let Some(opt_end) = after_open.find(']') else {
            return Err("pin options are missing a closing ]".to_string());
        };
        let package = rest[..opt_start].to_string();
        let opts_raw = &after_open[..opt_end];
        let after = &after_open[opt_end + 1..];
        let version = after
            .strip_prefix('@')
            .ok_or_else(|| "pin options must be followed by @version".to_string())?
            .to_string();
        return Ok((package, parse_options(opts_raw), version));
    }
    let version_at = if let Some(after_scope) = rest.strip_prefix('@') {
        after_scope.rfind('@').map(|i| i + 1)
    } else {
        rest.rfind('@')
    };
    let Some(at) = version_at else {
        return Err("pin is missing @version".to_string());
    };
    Ok((
        rest[..at].to_string(),
        BTreeMap::new(),
        rest[at + 1..].to_string(),
    ))
}

fn parse_options(raw: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((k, v)) = part.split_once('=') {
            out.insert(k.trim().to_string(), v.trim().to_string());
        } else {
            out.insert(part.to_string(), String::new());
        }
    }
    out
}

pub fn argv0(command: &str) -> &str {
    let token = command.split_whitespace().next().unwrap_or(command);
    Path::new(token)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(token)
}

pub fn is_mise_argv0(argv0: &str) -> bool {
    matches!(argv0, "mise" | "mise.exe")
}

#[cfg(test)]
#[path = "spec_tests.rs"]
mod tests;
