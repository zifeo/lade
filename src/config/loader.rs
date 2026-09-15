use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use semver::{Version, VersionReq};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use super::{Config, patterns::CompiledPatterns, secret::LadeRule};

/// One mapping, or a list of mappings when `.when` differs.
#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum RuleBodies {
    Many(Vec<LadeRule>),
    One(LadeRule),
}

impl RuleBodies {
    fn into_rules(self, pattern: &str) -> Result<Vec<LadeRule>> {
        match self {
            RuleBodies::One(rule) => Ok(vec![rule]),
            RuleBodies::Many(rules) => {
                if rules.is_empty() {
                    bail!("pattern '{pattern}' has an empty rule list");
                }
                Ok(rules)
            }
        }
    }
}

#[derive(Deserialize, Debug)]
struct RawLadeFile {
    #[serde(flatten)]
    commands: IndexMap<String, RuleBodies>,
}

#[derive(Debug)]
pub struct LadeFile {
    pub commands: IndexMap<String, Vec<LadeRule>>,
}

impl LadeFile {
    pub fn from_path(path: &Path) -> Result<LadeFile> {
        let raw = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()))?;
        let (req, mut config) = parse_lade_yaml(&raw)?;
        if let Some(req) = req.as_deref() {
            require_lade_version(req, path)?;
        }
        config.apply_merge()?;
        let raw: RawLadeFile = serde_yaml::from_value(config)?;
        let mut commands = IndexMap::new();
        for (pattern, bodies) in raw.commands {
            commands.insert(pattern.clone(), bodies.into_rules(&pattern)?);
        }
        Ok(LadeFile { commands })
    }

    pub fn build(path: PathBuf) -> Result<Config> {
        let files = yaml_files_on_walk(&path)?;
        let mut configs: Vec<(PathBuf, LadeFile)> = Vec::default();
        for file in files {
            let dir = file
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| path.clone());
            configs.push((
                dir,
                LadeFile::from_path(&file)
                    .with_context(|| format!("failed to parse {}", file.display()))?,
            ));
        }

        let mut rules = Vec::default();
        let mut regex_strs = Vec::default();
        configs.reverse();
        for (path, config) in configs.into_iter() {
            for (pattern, rule_list) in config.commands.into_iter() {
                for rule in rule_list {
                    regex_strs.push(pattern.clone());
                    rules.push((path.clone(), rule));
                }
            }
        }

        let compiled = CompiledPatterns::compile(&regex_strs)?;
        Ok(Config::new(rules, regex_strs, compiled))
    }
}

pub(crate) fn at_user_home(path: &Path) -> bool {
    directories::UserDirs::new().is_some_and(|u| u.home_dir() == path)
}

pub(crate) fn config_in_dir(dir: &Path) -> Result<Option<PathBuf>> {
    let yaml = dir.join("lade.yaml");
    let yml = dir.join("lade.yml");
    match (yaml.is_file(), yml.is_file()) {
        (true, true) => {
            bail!(
                "both lade.yaml and lade.yml exist in {}. Keep lade.yaml and remove lade.yml.",
                dir.display()
            );
        }
        (true, false) => Ok(Some(yaml)),
        (false, true) => Ok(Some(yml)),
        (false, false) => Ok(None),
    }
}

pub(crate) fn yaml_files_on_walk(start: &Path) -> Result<Vec<PathBuf>> {
    let mut path = start.to_path_buf();
    let mut out = Vec::new();
    loop {
        if let Some(found) = config_in_dir(&path)? {
            out.push(found);
        }
        if at_user_home(&path) {
            break;
        }
        match path.parent() {
            Some(parent) => path = parent.to_path_buf(),
            None => break,
        }
    }
    Ok(out)
}

/// Optional first line `#: >=0.18.0`. A YAML comment, so it cannot be a
/// command regex. A rule for `#` is a quoted key (`"#"` or `"\\#"`).
pub(crate) fn parse_lade_yaml(raw: &str) -> Result<(Option<String>, serde_yaml::Value)> {
    let (req, yaml_src) = take_version_line(raw)?;
    if yaml_src.trim().is_empty() {
        return Ok((req, serde_yaml::Value::Mapping(serde_yaml::Mapping::new())));
    }
    let value: serde_yaml::Value = serde_yaml::from_str(yaml_src).context("invalid YAML")?;
    if !value.is_mapping() {
        bail!("lade.yaml body must be a mapping");
    }
    Ok((req, value))
}

pub(crate) fn render_lade_yaml(req: Option<&str>, mapping: &serde_yaml::Value) -> Result<String> {
    let body = serde_yaml::to_string(mapping)?;
    match req {
        Some(req) => Ok(format!("#: {req}\n{body}")),
        None => Ok(body),
    }
}

fn take_version_line(raw: &str) -> Result<(Option<String>, &str)> {
    let body = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let (first, rest) = match body.split_once('\n') {
        Some((first, rest)) => (first.strip_suffix('\r').unwrap_or(first), rest),
        None => (body.strip_suffix('\r').unwrap_or(body), ""),
    };
    let trimmed = first.trim();
    let Some(after_hash) = trimmed.strip_prefix('#') else {
        return Ok((None, body));
    };
    let after_hash = after_hash.trim_start();
    let Some(req) = after_hash.strip_prefix(':') else {
        return Ok((None, body));
    };
    let req = req.trim();
    if req.is_empty() {
        bail!("lade.yaml `#:` version is empty. Example: `#: >=0.18.0`");
    }
    Ok((Some(req.to_string()), rest))
}

fn current_lade_version() -> Version {
    let parsed =
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0));
    Version::new(parsed.major, parsed.minor, parsed.patch)
}

pub(crate) fn require_lade_version(req: &str, path: &Path) -> Result<()> {
    let parsed = VersionReq::parse(req).with_context(|| {
        format!(
            "{} version `{req}` is not a semver range. Example: `#: >=0.18.0`",
            path.display()
        )
    })?;
    let current = current_lade_version();
    if parsed.matches(&current) {
        return Ok(());
    }
    bail!(
        "{} needs Lade {req}. This binary is {}.\nRun `lade upgrade`, or edit the `#:` line at the top of that file.",
        path.display(),
        env!("CARGO_PKG_VERSION")
    );
}

pub(crate) fn report_load_error(e: &anyhow::Error) {
    let text = format!("{e:#}");
    let title = if text.contains("needs Lade") {
        "This Lade is too old for this repo."
    } else {
        "Could not parse a lade.yaml."
    };
    crate::message_box::MessageBox::new()
        .error()
        .line(title)
        .line("")
        .paragraph(text)
        .line("")
        .line("Walk starts at this directory and stops at $HOME.")
        .print_stderr();
}
