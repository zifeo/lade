use std::collections::{BTreeSet, HashMap, VecDeque};

use anyhow::{Result, bail};
use once_cell::sync::Lazy;
use regex::Regex;

static VAR: Lazy<Regex> = Lazy::new(|| Regex::new(r"\$(?:\{(\w+)\}|(\w+))").unwrap());
static TEMPLATE_VAR: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\$(?:\{(\.?[A-Za-z_][A-Za-z0-9_]*)\}|([A-Za-z_][A-Za-z0-9_]*))").unwrap()
});

/// A source value compiled once for dependency discovery and interpolation.
///
/// Only braced references form graph edges. This intentionally leaves `$NAME`
/// intact for shell-backed providers, where the shell owns that syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template {
    parts: Vec<TemplatePart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TemplatePart {
    Literal(String),
    Reference {
        name: String,
        braced: bool,
        private: bool,
    },
}

impl Template {
    pub fn parse(value: &str) -> Self {
        let mut parts = Vec::new();
        let mut offset = 0;
        for captures in TEMPLATE_VAR.captures_iter(value) {
            let matched = captures.get(0).expect("regex match");
            if matched.start() > offset {
                parts.push(TemplatePart::Literal(
                    value[offset..matched.start()].to_string(),
                ));
            }
            let braced = captures.get(1).is_some();
            let raw_name = captures
                .get(1)
                .or_else(|| captures.get(2))
                .expect("reference name")
                .as_str();
            parts.push(TemplatePart::Reference {
                name: raw_name.trim_start_matches('.').to_string(),
                braced,
                private: raw_name.starts_with('.'),
            });
            offset = matched.end();
        }
        if offset < value.len() || parts.is_empty() {
            parts.push(TemplatePart::Literal(value[offset..].to_string()));
        }
        Self { parts }
    }

    pub fn dependencies(&self) -> impl Iterator<Item = &str> {
        self.parts.iter().filter_map(|part| match part {
            TemplatePart::Reference { name, .. } => Some(name.as_str()),
            TemplatePart::Literal(_) => None,
        })
    }

    pub fn render(&self, values: &HashMap<String, String>) -> Result<String> {
        let mut output = String::new();
        for part in &self.parts {
            match part {
                TemplatePart::Literal(value) => output.push_str(value),
                TemplatePart::Reference { name, braced, .. } => {
                    if let Some(value) = values.get(name) {
                        output.push_str(value);
                    } else if *braced {
                        bail!("missing dependency '{name}'");
                    } else {
                        output.push('$');
                        output.push_str(name);
                    }
                }
            }
        }
        Ok(output)
    }

    pub fn shell_source(&self) -> String {
        let mut output = String::new();
        for part in &self.parts {
            match part {
                TemplatePart::Literal(value) => output.push_str(value),
                TemplatePart::Reference {
                    name,
                    braced,
                    private,
                } => {
                    if *braced || *private {
                        output.push_str("${");
                        output.push_str(name);
                        output.push('}');
                    } else {
                        output.push('$');
                        output.push_str(name);
                    }
                }
            }
        }
        output
    }
}

/// A validated dependency graph. The execution owner chooses how to run ready
/// nodes, while this type keeps planning deterministic and I/O-free.
#[derive(Debug, Clone)]
pub struct Dag {
    templates: HashMap<String, Template>,
    dependents: HashMap<String, Vec<String>>,
    indegrees: HashMap<String, usize>,
}

impl Dag {
    pub fn new(templates: HashMap<String, Template>) -> Result<Self> {
        let mut dependents = HashMap::<String, Vec<String>>::new();
        let mut indegrees = HashMap::<String, usize>::new();
        for (name, template) in &templates {
            let mut dependencies = BTreeSet::new();
            for part in &template.parts {
                let TemplatePart::Reference { name, braced, .. } = part else {
                    continue;
                };
                if !templates.contains_key(name) {
                    if !braced {
                        continue;
                    }
                    bail!("binding '{name}' references missing dependency '{name}'");
                }
                dependencies.insert(name.clone());
            }
            indegrees.insert(name.clone(), dependencies.len());
            for dependency in dependencies {
                dependents.entry(dependency).or_default().push(name.clone());
            }
        }
        for values in dependents.values_mut() {
            values.sort();
        }
        let graph = Self {
            templates,
            dependents,
            indegrees,
        };
        graph.validate_acyclic()?;
        Ok(graph)
    }

    pub fn template(&self, name: &str) -> Option<&Template> {
        self.templates.get(name)
    }

    pub fn initial_ready(&self) -> Vec<String> {
        let mut ready = self
            .indegrees
            .iter()
            .filter_map(|(name, degree)| (*degree == 0).then_some(name.clone()))
            .collect::<Vec<_>>();
        ready.sort();
        ready
    }

    pub fn dependents(&self, name: &str) -> &[String] {
        self.dependents
            .get(name)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }

    pub fn indegrees(&self) -> HashMap<String, usize> {
        self.indegrees.clone()
    }

    fn validate_acyclic(&self) -> Result<()> {
        let mut indegrees = self.indegrees();
        let mut ready = self.initial_ready().into_iter().collect::<VecDeque<_>>();
        let mut visited = 0;
        while let Some(name) = ready.pop_front() {
            visited += 1;
            for dependent in self.dependents(&name) {
                let degree = indegrees
                    .get_mut(dependent)
                    .expect("dependent must have an indegree");
                *degree -= 1;
                if *degree == 0 {
                    ready.push_back(dependent.clone());
                }
            }
        }
        if visited == self.templates.len() {
            return Ok(());
        }
        let mut cycle = indegrees
            .into_iter()
            .filter_map(|(name, degree)| (degree > 0).then_some(name))
            .collect::<Vec<_>>();
        cycle.sort();
        bail!("cyclic binding dependencies: {}", cycle.join(", "))
    }
}

pub fn resolve(
    kvs: &HashMap<String, String>,
    existing_vars: &HashMap<String, String>,
) -> Result<HashMap<String, String>> {
    kvs.iter()
        .map(|(key, value)| resolve_one(value, existing_vars).map(|v| (key.clone(), v)))
        .collect()
}

pub fn resolve_one(value: &str, existing_vars: &HashMap<String, String>) -> Result<String> {
    Ok(VAR
        .replace_all(value, |caps: &regex::Captures| {
            let name = caps
                .get(1)
                .or_else(|| caps.get(2))
                .map(|m| m.as_str())
                .unwrap_or("");
            existing_vars.get(name).cloned().unwrap_or_default()
        })
        .into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_resolve_one_no_vars() {
        assert_eq!(
            resolve_one("hello world", &HashMap::new()).unwrap(),
            "hello world"
        );
    }

    #[test]
    fn test_resolve_one_dollar_var() {
        let vars = HashMap::from([("FOO".to_string(), "bar".to_string())]);
        assert_eq!(resolve_one("prefix_$FOO", &vars).unwrap(), "prefix_bar");
    }

    #[test]
    fn test_resolve_one_braces_var() {
        let vars = HashMap::from([("FOO".to_string(), "bar".to_string())]);
        assert_eq!(
            resolve_one("prefix_${FOO}_suffix", &vars).unwrap(),
            "prefix_bar_suffix"
        );
    }

    #[test]
    fn test_resolve_one_multiple_vars() {
        let vars = HashMap::from([
            ("A".to_string(), "hello".to_string()),
            ("B".to_string(), "world".to_string()),
        ]);
        assert_eq!(resolve_one("$A $B", &vars).unwrap(), "hello world");
    }

    #[test]
    fn test_resolve_one_unknown_var_empty() {
        assert_eq!(
            resolve_one("val/$MISSING", &HashMap::new()).unwrap(),
            "val/"
        );
    }

    #[test]
    fn test_resolve_one_adjacent_braced_vars() {
        let vars = HashMap::from([
            ("A".to_string(), "foo".to_string()),
            ("B".to_string(), "bar".to_string()),
        ]);
        assert_eq!(resolve_one("${A}${B}", &vars).unwrap(), "foobar");
    }

    #[test]
    fn test_resolve_one_word_boundary_without_braces() {
        let vars = HashMap::from([("FOO".to_string(), "bar".to_string())]);
        assert_eq!(resolve_one("$FOO_SUFFIX", &vars).unwrap(), "");
    }

    #[test]
    fn test_resolve_one_no_double_expansion() {
        // A value that itself looks like a variable reference must not be re-expanded.
        let vars = HashMap::from([
            ("A".into(), "$B".into()),
            ("B".into(), "should_not_appear".into()),
        ]);
        assert_eq!(resolve_one("$A", &vars).unwrap(), "$B");
    }

    #[test]
    fn test_resolve_one_unmatched_open_brace_is_literal() {
        // "${FOO" has no closing brace — must not be treated as a variable reference.
        let vars = HashMap::from([("FOO".to_string(), "bar".to_string())]);
        assert_eq!(resolve_one("${FOO", &vars).unwrap(), "${FOO");
    }

    #[test]
    fn test_resolve_one_trailing_brace_not_consumed() {
        // "$FOO}" — only "$FOO" is a variable reference; the "}" is literal.
        let vars = HashMap::from([("FOO".to_string(), "bar".to_string())]);
        assert_eq!(resolve_one("$FOO}", &vars).unwrap(), "bar}");
    }

    #[test]
    fn test_resolve_batch() {
        let kvs = HashMap::from([
            ("URL".to_string(), "https://$HOST/api".to_string()),
            ("STATIC".to_string(), "literal".to_string()),
        ]);
        let vars = HashMap::from([("HOST".to_string(), "example.com".to_string())]);
        let result = resolve(&kvs, &vars).unwrap();
        assert_eq!(result.get("URL").unwrap(), "https://example.com/api");
        assert_eq!(result.get("STATIC").unwrap(), "literal");
    }

    #[test]
    fn template_tracks_braced_and_bare_references() {
        let template = Template::parse("sh://echo $HOME ${TOKEN}");
        assert_eq!(
            template.dependencies().collect::<Vec<_>>(),
            vec!["HOME", "TOKEN"]
        );
        assert_eq!(
            template
                .render(&HashMap::from([("TOKEN".into(), "value".into())]))
                .unwrap(),
            "sh://echo $HOME value"
        );
    }

    #[test]
    fn dag_allows_unknown_bare_shell_variables() {
        Dag::new(HashMap::from([(
            "AUTHORIZATION".into(),
            Template::parse("sh://echo $HOME"),
        )]))
        .unwrap();
    }

    #[test]
    fn template_normalizes_private_reference_for_shell() {
        let template = Template::parse("sh://echo ${.TOKEN}");
        assert_eq!(template.dependencies().collect::<Vec<_>>(), vec!["TOKEN"]);
        assert_eq!(template.shell_source(), "sh://echo ${TOKEN}");
    }

    #[test]
    fn dag_rejects_missing_dependency() {
        let err = Dag::new(HashMap::from([(
            "HEADER".into(),
            Template::parse("Bearer ${TOKEN}"),
        )]))
        .unwrap_err();
        assert!(err.to_string().contains("missing dependency 'TOKEN'"));
    }

    #[test]
    fn dag_rejects_cycles() {
        let err = Dag::new(HashMap::from([
            ("A".into(), Template::parse("${B}")),
            ("B".into(), Template::parse("${A}")),
        ]))
        .unwrap_err();
        assert!(err.to_string().contains("cyclic binding dependencies"));
    }
}
