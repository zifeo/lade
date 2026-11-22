use anyhow::{Context, Result, bail};
use indexmap::IndexMap;
use semver::{Version, VersionReq};
use serde::Deserialize;
use std::path::{Path, PathBuf};

use super::{Config, patterns::CompiledPatterns, secret::LadeRule};

/// One mapping, or a list of mappings when `.when` differs.
#[derive(Debug)]
enum RuleBodies {
    Many(Vec<LadeRule>),
    One(LadeRule),
}

// An untagged enum would replace a body's own error, such as the unknown `.`
// key it names, with "did not match any variant".
impl<'de> Deserialize<'de> for RuleBodies {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // § t4c9tbype@nj39h
        let value = serde_yaml::Value::deserialize(deserializer)?;
        match value {
            serde_yaml::Value::Sequence(bodies) => bodies
                .into_iter()
                .map(|body| serde_yaml::from_value(string_keys(body)))
                .collect::<std::result::Result<_, _>>()
                .map(RuleBodies::Many),
            body => serde_yaml::from_value(string_keys(body)).map(RuleBodies::One),
        }
        .map_err(serde::de::Error::custom)
    }
}

/// A body's struct fields are read by name, so a number key such as a
/// tunnel's port `1223` must reach them as text.
fn string_keys(body: serde_yaml::Value) -> serde_yaml::Value {
    let serde_yaml::Value::Mapping(map) = body else {
        return body;
    };
    serde_yaml::Value::Mapping(
        map.into_iter()
            .map(|(key, value)| match key {
                serde_yaml::Value::Number(n) => (serde_yaml::Value::String(n.to_string()), value),
                key => (key, value),
            })
            .collect(),
    )
}

impl RuleBodies {
    fn into_rules(self, pattern: &str) -> Result<Vec<LadeRule>> {
        // § v03g22hz0.b@hn4d6
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
        Self::from_text(&raw, path)
    }

    /// `raw` as the content of `path`, which names the file in errors.
    pub(crate) fn from_text(raw: &str, path: &Path) -> Result<LadeFile> {
        let (req, mut config) = parse_lade_yaml(raw)?;
        if let Some(req) = req.as_deref() {
            // § bvr6wk206.b@52823
            require_lade_version(req, path)?;
        }
        config.apply_merge()?;
        let raw: RawLadeFile = serde_yaml::from_value(config)?;
        let mut commands = IndexMap::new();
        for (pattern, bodies) in raw.commands {
            if pattern.is_empty() {
                // § jj7875tr3.a@2mqx1
                bail!("`: ` is the Lade version, not a command. Use `.` to match every command.");
            }
            commands.insert(pattern.clone(), bodies.into_rules(&pattern)?);
        }
        Ok(LadeFile { commands })
    }

    /// Compiles every key as a load of the walk does.
    pub(crate) fn check_patterns(&self) -> Result<()> {
        CompiledPatterns::compile(&self.commands.keys().cloned().collect::<Vec<_>>())?;
        Ok(())
    }

    pub fn build(path: PathBuf) -> Result<Config> {
        // § je3g2tk0d.b@nr92j
        let files = yaml_files_on_walk(&path)?;
        let walk_hash = super::hash_yaml_files(&files);
        // § end je3g2tk0d.b
        crate::remote::set_walk(walk_hash);
        let mut configs: Vec<(PathBuf, LadeFile)> = Vec::default();
        for file in &files {
            // § y4zs3xgg4.c@tetbf
            let dir = file
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| path.clone());
            configs.push((
                dir,
                LadeFile::from_path(file)
                    .and_then(|lade| lade.check_patterns().map(|()| lade))
                    .with_context(|| format!("failed to parse {}", file.display()))?,
            ));
        }

        let mut rules = Vec::default();
        let mut regex_strs = Vec::default();
        // § sn2x1h4v9@rwbpk
        configs.reverse();
        for (dir, config) in configs.into_iter() {
            for (pattern, rule_list) in config.commands.into_iter() {
                // § sn2x1h4v9.a@w1gv1
                for rule in rule_list {
                    regex_strs.push(pattern.clone());
                    rules.push((dir.clone(), rule));
                }
            }
        }
        // § end sn2x1h4v9

        let compiled = CompiledPatterns::compile(&regex_strs)?;
        Ok(Config::new(rules, regex_strs, compiled, walk_hash, path))
    }
}

/// `$HOME` may be reached through a symlink (`/var` is `/private/var` on macOS),
/// so the walk compares real paths.
fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn canonical_home() -> Option<PathBuf> {
    crate::home_dir().map(|home| canonical(&home))
}

pub(crate) fn at_user_home(path: &Path) -> bool {
    canonical_home().is_some_and(|home| home == canonical(path))
}

pub(crate) fn config_in_dir(dir: &Path) -> Result<Option<PathBuf>> {
    let yaml = dir.join("lade.yaml");
    let yml = dir.join("lade.yml");
    match (yaml.is_file(), yml.is_file()) {
        (true, true) => {
            // § y4zs3xgg4.b@t7hvd
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
    // § y4zs3xgg4.a@cj835
    let home = canonical_home();
    let mut path = start.to_path_buf();
    let mut out = Vec::new();
    loop {
        let here = canonical(&path);
        if home
            .as_ref()
            .is_some_and(|home| *home != here && home.starts_with(&here))
        {
            break;
        }
        if let Some(found) = config_in_dir(&path)? {
            out.push(found);
        }
        if home.as_ref() == Some(&here) {
            break;
        }
        match path.parent() {
            Some(parent) => path = parent.to_path_buf(),
            None => break,
        }
    }
    Ok(out)
}

/// Optional empty-key range (`: >=0.18.0`). Empty regex is not a command.
/// Use `.` to match every command. The line is peeled before YAML parse:
/// a leading `: range` is not valid YAML as a mapping key.
pub(crate) fn parse_lade_yaml(raw: &str) -> Result<(Option<String>, serde_yaml::Value)> {
    // § bvr6wk206.a@p3djj
    let (line_req, yaml_src) = take_version_line(raw)?;
    if yaml_src.trim().is_empty() {
        return Ok((
            line_req,
            serde_yaml::Value::Mapping(serde_yaml::Mapping::new()),
        ));
    }
    let mut value: serde_yaml::Value = serde_yaml::from_str(yaml_src).context("invalid YAML")?;
    if !value.is_mapping() {
        bail!("lade.yaml body must be a mapping");
    }
    let key_req = take_version_key(value.as_mapping_mut().expect("mapping"))?;
    let req = match (line_req, key_req) {
        (None, None) => None,
        (Some(req), None) | (None, Some(req)) => Some(req),
        (Some(_), Some(_)) => {
            bail!("lade.yaml has two versions. Keep one `: >=0.18.0`.");
        }
    };
    Ok((req, value))
}

fn take_version_line(raw: &str) -> Result<(Option<String>, &str)> {
    let body = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    let (first, rest) = match body.split_once('\n') {
        Some((first, rest)) => (first.strip_suffix('\r').unwrap_or(first), rest),
        None => (body.strip_suffix('\r').unwrap_or(body), ""),
    };
    let trimmed = first.trim();
    if trimmed.starts_with('#') {
        return Ok((None, body));
    }
    let Some(after) = trimmed.strip_prefix(':') else {
        return Ok((None, body));
    };
    let after = unquote_version(after.trim());
    if after.is_empty() {
        if rest.starts_with(' ') || rest.starts_with('\t') {
            // § jj7875tr3.a@96w3t
            bail!("`: ` is the Lade version, not a command. Use `.` to match every command.");
        }
        bail!("lade.yaml `:` version is empty. Example: `: >=0.18.0`");
    }
    if after.contains(':') {
        return Ok((None, body));
    }
    Ok((Some(after.to_string()), rest))
}

fn unquote_version(s: &str) -> &str {
    for quote in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(quote) && s.ends_with(quote) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

fn take_version_key(map: &mut serde_yaml::Mapping) -> Result<Option<String>> {
    let null_val = map.remove(serde_yaml::Value::Null);
    let empty_val = map.remove(serde_yaml::Value::String(String::new()));
    let value = match (null_val, empty_val) {
        (None, None) => return Ok(None),
        (Some(value), None) | (None, Some(value)) => value,
        (Some(_), Some(_)) => {
            bail!("lade.yaml has both `:` and `\"\":`. Keep one version: `: >=0.18.0`.");
        }
    };
    version_from_value(value)
}

fn version_from_value(value: serde_yaml::Value) -> Result<Option<String>> {
    match value {
        serde_yaml::Value::String(req) => {
            let req = req.trim();
            if req.is_empty() {
                bail!("lade.yaml `:` version is empty. Example: `: >=0.18.0`");
            }
            Ok(Some(req.to_string()))
        }
        serde_yaml::Value::Null => {
            bail!("lade.yaml `:` version is empty. Example: `: >=0.18.0`");
        }
        serde_yaml::Value::Mapping(_) | serde_yaml::Value::Sequence(_) => {
            // § jj7875tr3.a@3ja00
            bail!("`: ` is the Lade version, not a command. Use `.` to match every command.");
        }
        other => {
            bail!(
                "lade.yaml `:` version must be a semver range. Example: `: >=0.18.0`. Got {other:?}."
            );
        }
    }
}

fn current_lade_version() -> Version {
    let parsed =
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or_else(|_| Version::new(0, 0, 0));
    Version::new(parsed.major, parsed.minor, parsed.patch)
}

pub(crate) fn require_lade_version(req: &str, path: &Path) -> Result<()> {
    // § bvr6wk206.a@cw99n
    let parsed = VersionReq::parse(req).with_context(|| {
        format!(
            "{} version `{req}` is not a semver range. Example: `: >=0.18.0`",
            path.display()
        )
    })?;
    // § end bvr6wk206.a
    let current = current_lade_version();
    if parsed.matches(&current) {
        return Ok(());
    }
    bail!(
        "{} needs Lade {req}. This binary is {}.\nRun `lade upgrade`, or edit the `:` version in that file.",
        path.display(),
        env!("CARGO_PKG_VERSION")
    );
}

pub(crate) fn report_load_error(e: &anyhow::Error) {
    load_error_box(e).print_stderr();
}

/// The box of a walk that does not load, for a door to add its way out to.
pub(crate) fn load_error_box(e: &anyhow::Error) -> crate::message_box::MessageBox {
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
}
