use anyhow::{Context, Result, bail};
use serde_yaml::Value;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::LadeFile;
use crate::message_box::Report;

use super::ask;
use super::edit;

pub fn nearest_yaml() -> Result<Option<PathBuf>> {
    Ok(
        crate::config::yaml_files_on_walk(&std::env::current_dir()?)?
            .into_iter()
            .next(),
    )
}

pub fn target_yaml(tty: bool) -> Result<PathBuf> {
    // § qp8tmdhae.a@parh7
    let found = crate::config::yaml_files_on_walk(&std::env::current_dir()?)?;
    match found.as_slice() {
        [] => Ok(std::env::current_dir()?.join("lade.yaml")),
        [one] => Ok(one.clone()),
        many if tty => {
            let mut report = Report::new().heading("lade.yaml on the walk");
            for (i, path) in many.iter().enumerate() {
                report = report.line(format!("  {}. {}", i + 1, path.display()));
            }
            report.print();
            let answer = ask("Which (number, 1 is nearest): ")?;
            let index: usize = answer.parse().context("pick a number from the list")?;
            many.get(index.saturating_sub(1))
                .cloned()
                .context("pick a number from the list")
        }
        many => Ok(many[0].clone()),
    }
}

pub fn upsert_binding(path: &Path, rule: &str, key: &str, uri: &str) -> Result<()> {
    let mut file = load_file(path)?;
    let existing = file
        .root
        .as_mapping()
        .and_then(|map| map.get(Value::String(rule.to_string())))
        .cloned();
    {
        let map = file
            .root
            .as_mapping_mut()
            .context("lade.yaml root must be a mapping")?;
        let rule_key = Value::String(rule.to_string());
        let entry = map
            .entry(rule_key)
            .or_insert_with(|| Value::Mapping(serde_yaml::Mapping::new()));
        let rule_map = match entry {
            Value::Mapping(m) => m,
            Value::Sequence(seq) => {
                if seq.is_empty() {
                    *entry = Value::Mapping(serde_yaml::Mapping::new());
                    entry.as_mapping_mut().expect("just set mapping")
                } else {
                    bail!("rule `{rule}` is a list. Edit lade.yaml by hand.");
                }
            }
            _ => bail!("rule `{rule}` is not a mapping"),
        };
        rule_map.insert(
            Value::String(key.to_string()),
            Value::String(uri.to_string()),
        );
    }
    let mut doc = edit::Doc::new(&file.raw);
    doc.upsert(existing.as_ref(), rule, key, uri)
        .with_context(|| cannot_edit(path))?;
    let text = doc.render();
    check_edit(path, &text, &file)?;
    // § qp8tmdhae.c@pvt7m
    LadeFile::from_text(&text, path)?.check_patterns()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, text)?;
    Ok(())
}

pub fn drop_binding(path: &Path, rule: &str, key: &str) -> Result<Option<String>> {
    let mut file = load_file(path)?;
    let (removed, drop_rule) = {
        let Some(map) = file.root.as_mapping_mut() else {
            bail!("lade.yaml root must be a mapping");
        };
        let rule_key = Value::String(rule.to_string());
        let Some(entry) = map.get_mut(&rule_key) else {
            // § qp8tmdhae.d@6mk0k
            return Ok(None);
        };
        let Some(rule_map) = entry.as_mapping_mut() else {
            bail!("rule `{rule}` is not a mapping");
        };
        let Some(removed) = rule_map.remove(Value::String(key.to_string())) else {
            // § qp8tmdhae.d@bypfr
            return Ok(None);
        };
        let drop_rule = rule_map.is_empty();
        if drop_rule {
            map.remove(&rule_key);
        }
        let removed = match removed {
            Value::String(uri) => uri,
            _ => String::new(),
        };
        (removed, drop_rule)
    };
    let mut doc = edit::Doc::new(&file.raw);
    doc.drop(rule, key, drop_rule)
        .with_context(|| cannot_edit(path))?;
    save_file(path, &doc.render(), &file)?;
    Ok(Some(removed))
}

pub fn replace_binding_uri(path: &Path, key: &str, uri: &str) -> Result<bool> {
    let mut file = load_file(path)?;
    let rules = edit::rules_binding(&file.root, key, |_| true);
    set_in_rules(path, &mut file, &rules, key, uri)?;
    Ok(!rules.is_empty())
}

/// Replaces `key` only where it is bound to `from`, so a file that binds the key to
/// another value keeps it.
pub fn replace_binding_value(path: &Path, key: &str, from: &str, to: &str) -> Result<bool> {
    let mut file = load_file(path)?;
    let rules = edit::rules_binding(&file.root, key, |value| value.as_str() == Some(from));
    set_in_rules(path, &mut file, &rules, key, to)?;
    Ok(!rules.is_empty())
}

fn set_in_rules(
    path: &Path,
    file: &mut YamlFile,
    rules: &[String],
    key: &str,
    uri: &str,
) -> Result<()> {
    if rules.is_empty() {
        return Ok(());
    }
    let mut doc = edit::Doc::new(&file.raw);
    for rule in rules {
        if let Some(rule_map) = file
            .root
            .get_mut(Value::String(rule.clone()))
            .and_then(Value::as_mapping_mut)
        {
            rule_map.insert(
                Value::String(key.to_string()),
                Value::String(uri.to_string()),
            );
        }
        doc.set(rule, key, uri).with_context(|| cannot_edit(path))?;
    }
    save_file(path, &doc.render(), file)
}

struct YamlFile {
    raw: String,
    req: Option<String>,
    root: Value,
}

fn load_file(path: &Path) -> Result<YamlFile> {
    if !path.exists() {
        return Ok(YamlFile {
            raw: String::new(),
            req: None,
            root: Value::Mapping(serde_yaml::Mapping::new()),
        });
    }
    let raw = fs::read_to_string(path)?;
    let (req, mapping) = crate::config::parse_lade_yaml(&raw)?;
    if let Some(req_str) = req.as_deref() {
        // § bvr6wk206.b@erh0q
        // silcrow:follow
        crate::config::require_lade_version(req_str, path)?;
    }
    if !mapping.is_mapping() {
        bail!("{} is not a mapping", path.display());
    }
    Ok(YamlFile {
        raw,
        req,
        root: mapping,
    })
}

fn cannot_edit(path: &Path) -> String {
    format!(
        "Lade edits {} line by line and cannot here. Edit it by hand.",
        path.display()
    )
}

/// `text` must read back as the file `file` now models, the version pin included.
fn check_edit(path: &Path, text: &str, file: &YamlFile) -> Result<()> {
    // § qp8tmdhae.f@wphzf
    let (req, root) = crate::config::parse_lade_yaml(text).with_context(|| cannot_edit(path))?;
    // § one qp8tmdhae.b@gz37w
    if req != file.req {
        bail!(
            "{} The line edit would change the version pin.",
            cannot_edit(path)
        );
    }
    if root != file.root {
        bail!(
            "{} The line edit would change more than the binding.",
            cannot_edit(path)
        );
    }
    Ok(())
}

fn save_file(path: &Path, text: &str, file: &YamlFile) -> Result<()> {
    check_edit(path, text, file)?;
    fs::write(path, text)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn upsert_then_remove_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        upsert_binding(&path, "^terraform", "TF_VAR_FOO", "op://v/i/f").unwrap();
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("TF_VAR_FOO"));
        assert!(body.contains("op://v/i/f"));
        assert!(
            drop_binding(&path, "^terraform", "TF_VAR_FOO")
                .unwrap()
                .is_some()
        );
        let body = fs::read_to_string(&path).unwrap();
        assert!(!body.contains("TF_VAR_FOO"));
    }

    #[test]
    fn both_extensions_error() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("lade.yaml"), ".\n  x: y\n").unwrap();
        fs::write(dir.path().join("lade.yml"), ".\n  x: y\n").unwrap();
        let err = crate::config::config_in_dir(dir.path()).unwrap_err();
        assert!(err.to_string().contains("both lade.yaml and lade.yml"));
    }

    #[test]
    fn replace_binding_value_leaves_another_value() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lade.yml");
        std::fs::write(
            &path,
            "^jq:\n  jq: mise://aqua/jqlang/jq@latest\n^other:\n  jq: mise://aqua/jqlang/jq@1.6.0\n",
        )
        .unwrap();
        assert!(
            !replace_binding_value(&path, "jq", "nope", "mise://aqua/jqlang/jq@1.7.1").unwrap()
        );
        assert!(
            replace_binding_value(
                &path,
                "jq",
                "mise://aqua/jqlang/jq@latest",
                "mise://aqua/jqlang/jq@1.7.1"
            )
            .unwrap()
        );
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("jq@1.7.1"), "{text}");
        assert!(text.contains("jq@1.6.0"), "{text}");
        assert!(!text.contains("@latest"), "{text}");
    }

    #[test]
    fn replace_binding_uri_updates_the_key() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        upsert_binding(&path, "^jq", "jq", "mise://aqua/jqlang/jq@latest").unwrap();
        assert!(replace_binding_uri(&path, "jq", "mise://aqua/jqlang/jq@1.7.1").unwrap());
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("1.7.1"), "{body}");
        assert!(!body.contains("@latest"), "{body}");
    }

    #[test]
    fn upsert_keeps_version() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        fs::write(
            &path,
            ": >=0.1.0\n\"^jq\":\n  jq: mise://aqua/jqlang/jq@1.7.0\n",
        )
        .unwrap();
        upsert_binding(&path, "^jq", "jq", "mise://aqua/jqlang/jq@1.7.1").unwrap();
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.starts_with(": >=0.1.0\n"), "{body}");
        assert!(!body.contains("---"), "{body}");
        assert!(body.contains("1.7.1"), "{body}");
    }

    #[test]
    fn drop_keeps_version() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        fs::write(
            &path,
            ": >=0.1.0\n\"^jq\":\n  jq: mise://aqua/jqlang/jq@1.7.0\n  K: v\n",
        )
        .unwrap();
        assert!(drop_binding(&path, "^jq", "K").unwrap().is_some());
        let body = fs::read_to_string(&path).unwrap();
        assert!(body.starts_with(": >=0.1.0\n"), "{body}");
        assert!(!body.contains("K: v"), "{body}");
    }

    #[test]
    fn upsert_refuses_too_new_pin() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        let before = ": >=99.0.0\n\"^jq\":\n  K: v\n";
        fs::write(&path, before).unwrap();
        let err = upsert_binding(&path, "^jq", "J", "w").unwrap_err();
        assert!(format!("{err:#}").contains(">=99.0.0"), "{err:#}");
        let err = drop_binding(&path, "^jq", "K").unwrap_err();
        assert!(format!("{err:#}").contains(">=99.0.0"), "{err:#}");
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn upsert_that_breaks_the_file_leaves_it_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        let before = "# kept\n\"^jq\":\n  K: v\n";
        fs::write(&path, before).unwrap();
        upsert_binding(&path, "^jq", ".", "not a body").unwrap_err();
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn upsert_with_a_rule_that_is_no_regex_leaves_the_file_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        let before = "# kept\n\"^jq\":\n  K: v\n";
        fs::write(&path, before).unwrap();
        upsert_binding(&path, "(", "K", "v").unwrap_err();
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn drop_of_an_absent_key_writes_nothing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        let before = "# kept\n\"^jq\":\n  K: v\n";
        fs::write(&path, before).unwrap();
        assert!(drop_binding(&path, "^jq", "MISSING").unwrap().is_none());
        assert!(drop_binding(&path, "^other", "K").unwrap().is_none());
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    const HAND: &str = concat!(
        ": >=0.1.0\n",
        "# Tools for the repo.\n",
        "\n",
        "\"^terraform\":   # infra\n",
        "  # the vault token\n",
        "  TF_VAR_TOKEN: op://v/i/token  # rotated monthly\n",
        "  'QUOTED': \"op://v/i/quoted\"\n",
        "\n",
        "  # keep last\n",
        "  LAST: op://v/i/last\n",
        "  # trailing inside\n",
        "\n",
        "^jq:\n",
        "    jq: mise://aqua/jqlang/jq@latest   # pinned by setup\n",
    );

    fn hand_file() -> (tempfile::TempDir, PathBuf) {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        fs::write(&path, HAND).unwrap();
        (dir, path)
    }

    fn hand_with(from: &str, to: &str) -> String {
        assert_eq!(HAND.matches(from).count(), 1, "{from}");
        HAND.replace(from, to)
    }

    #[test]
    fn add_of_a_new_key_inserts_one_line_after_the_rules_last_binding() {
        let (_dir, path) = hand_file();
        upsert_binding(&path, "^terraform", "NEW", "op://v/i/new").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            hand_with(
                "  LAST: op://v/i/last\n",
                "  LAST: op://v/i/last\n  NEW: op://v/i/new\n"
            )
        );
    }

    #[test]
    fn add_over_a_bound_key_rewrites_its_line_keeping_the_key_and_the_comment_beside() {
        let (_dir, path) = hand_file();
        upsert_binding(&path, "^terraform", "TF_VAR_TOKEN", "op://v/i/t2").unwrap();
        upsert_binding(&path, "^terraform", "QUOTED", "op://v/i/q2").unwrap();
        let want = hand_with(
            "  TF_VAR_TOKEN: op://v/i/token  # rotated monthly\n",
            "  TF_VAR_TOKEN: op://v/i/t2  # rotated monthly\n",
        )
        .replace(
            "  'QUOTED': \"op://v/i/quoted\"\n",
            "  'QUOTED': op://v/i/q2\n",
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), want);
    }

    #[test]
    fn add_of_a_new_rule_appends_it_at_the_files_binding_indent() {
        let (_dir, path) = hand_file();
        upsert_binding(&path, "^make", "TOKEN", "op://v/i/make").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            format!("{HAND}^make:\n  TOKEN: op://v/i/make\n")
        );
    }

    #[test]
    fn remove_drops_the_binding_lines_and_keeps_the_comments_around_them() {
        let (_dir, path) = hand_file();
        assert_eq!(
            drop_binding(&path, "^terraform", "TF_VAR_TOKEN").unwrap(),
            Some("op://v/i/token".to_string())
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            hand_with("  TF_VAR_TOKEN: op://v/i/token  # rotated monthly\n", "")
        );
    }

    #[test]
    fn remove_of_a_rules_last_binding_drops_the_rule_line_and_keeps_the_rest() {
        let (_dir, path) = hand_file();
        drop_binding(&path, "^jq", "jq").unwrap().unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            hand_with(
                "^jq:\n    jq: mise://aqua/jqlang/jq@latest   # pinned by setup\n",
                ""
            )
        );
    }

    #[test]
    fn a_pin_rewrite_changes_only_the_value_on_its_line() {
        let (_dir, path) = hand_file();
        assert!(
            replace_binding_value(
                &path,
                "jq",
                "mise://aqua/jqlang/jq@latest",
                "mise://aqua/jqlang/jq@1.7.1"
            )
            .unwrap()
        );
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            hand_with(
                "    jq: mise://aqua/jqlang/jq@latest   # pinned by setup\n",
                "    jq: mise://aqua/jqlang/jq@1.7.1   # pinned by setup\n"
            )
        );
    }

    #[test]
    fn an_edit_keeps_crlf_line_ends_and_a_missing_final_newline() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        fs::write(&path, "# kept\r\n^jq:\r\n  K: v # beside").unwrap();
        upsert_binding(&path, "^jq", "J", "w").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# kept\r\n^jq:\r\n  K: v # beside\r\n  J: w"
        );
        drop_binding(&path, "^jq", "K").unwrap().unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# kept\r\n^jq:\r\n  J: w"
        );
    }

    #[test]
    fn add_into_an_empty_rule_opens_its_block() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        fs::write(&path, "# kept\n'^jq': []\n^z:\n    Z: z\n").unwrap();
        upsert_binding(&path, "^jq", "K", "v").unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "# kept\n'^jq':\n    K: v\n^z:\n    Z: z\n"
        );
    }

    #[test]
    fn a_rule_in_flow_style_fails_the_edit_and_leaves_the_file_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        let before = "# kept\n^jq: {K: v}\n";
        fs::write(&path, before).unwrap();
        let err = format!("{:#}", upsert_binding(&path, "^jq", "J", "w").unwrap_err());
        assert!(err.contains("Edit it by hand"), "{err}");
        assert!(err.contains("flow style"), "{err}");
        let err = format!("{:#}", drop_binding(&path, "^jq", "K").unwrap_err());
        assert!(err.contains("flow style"), "{err}");
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn an_edit_that_reads_back_as_another_file_fails_and_leaves_it_unchanged() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("lade.yaml");
        let before = "^jq:\n  {K: v}\n";
        fs::write(&path, before).unwrap();
        let err = format!("{:#}", upsert_binding(&path, "^jq", "J", "w").unwrap_err());
        assert!(err.contains("Edit it by hand"), "{err}");
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }
}
