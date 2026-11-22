//! Line edits of a `lade.yaml`: each verb changes the lines of one binding and keeps
//! every other byte. `serde_yaml` decodes each key, and the caller reads the edited
//! text back, so a layout this module misreads fails the verb instead of writing.

use anyhow::{Context, Result, bail};
use serde_yaml::{Mapping, Value};

struct Line {
    text: String,
    end: String,
}

struct Entry {
    key: Option<Value>,
    start: usize,
    last: usize,
}

struct Level {
    indent: usize,
    entries: Vec<Entry>,
}

pub(super) struct Doc {
    bom: bool,
    eol: String,
    lines: Vec<Line>,
}

impl Doc {
    pub(super) fn new(raw: &str) -> Doc {
        let (bom, body) = match raw.strip_prefix('\u{feff}') {
            Some(body) => (true, body),
            None => (false, raw),
        };
        let lines: Vec<Line> = body
            .split_inclusive('\n')
            .map(|line| {
                let text = line.trim_end_matches(['\n', '\r']);
                Line {
                    text: text.to_string(),
                    end: line[text.len()..].to_string(),
                }
            })
            .collect();
        let eol = lines
            .iter()
            .map(|line| line.end.as_str())
            .find(|end| !end.is_empty())
            .unwrap_or("\n")
            .to_string();
        Doc { bom, eol, lines }
    }

    pub(super) fn render(&self) -> String {
        let mut out = String::new();
        if self.bom {
            out.push('\u{feff}');
        }
        for line in &self.lines {
            out.push_str(&line.text);
            out.push_str(&line.end);
        }
        out
    }

    /// Writes `key: uri` into `rule`, the rule's value in `before` being `existing`.
    pub(super) fn upsert(
        &mut self,
        existing: Option<&Value>,
        rule: &str,
        key: &str,
        uri: &str,
    ) -> Result<()> {
        // § qp8tmdhae.e@yyqxg
        let pair = format!("{}: {}", token(key)?, token(uri)?);
        let Some(existing) = existing else {
            let top = self.level(0, self.lines.len())?;
            let indent = " ".repeat(top.as_ref().map_or(0, |top| top.indent));
            let child = format!("{indent}{}{pair}", " ".repeat(self.child_step()?));
            let head = format!("{indent}{}:", token(rule)?);
            self.insert(self.lines.len(), vec![head, child]);
            return Ok(());
        };
        let rule_entry = self.rule(rule)?;
        if is_empty_collection(existing) {
            let (rule_indent, key_text) = self.key_text(&rule_entry, rule)?;
            let child = format!("{}{pair}", " ".repeat(rule_indent + self.child_step()?));
            let head = format!("{}{key_text}:", " ".repeat(rule_indent));
            self.replace(rule_entry.start, rule_entry.last, vec![head, child]);
            return Ok(());
        }
        let children = self.children(&rule_entry, rule)?;
        let wanted = Value::String(key.to_string());
        match children
            .entries
            .iter()
            .find(|e| e.key.as_ref() == Some(&wanted))
        {
            Some(child) => self.set_value(child, key, uri),
            None if existing
                .as_mapping()
                .is_some_and(|m| m.contains_key(&wanted)) =>
            {
                bail!("cannot find `{key}` of rule `{rule}` line by line")
            }
            None => {
                let after = children.entries.last().map_or(rule_entry.last, |e| e.last);
                let line = format!("{}{pair}", " ".repeat(children.indent));
                self.insert(after + 1, vec![line]);
                Ok(())
            }
        }
    }

    /// Drops `key` from `rule`, and the rule's key line when `drop_rule`.
    pub(super) fn drop(&mut self, rule: &str, key: &str, drop_rule: bool) -> Result<()> {
        // § qp8tmdhae.e@42f6f
        let (rule_entry, child) = self.binding(rule, key)?;
        self.lines.drain(child.start..=child.last);
        if drop_rule {
            self.lines.remove(rule_entry.start);
        }
        Ok(())
    }

    /// Sets the value of `key` in `rule`, which holds it.
    pub(super) fn set(&mut self, rule: &str, key: &str, uri: &str) -> Result<()> {
        // § qp8tmdhae.e@fxp9f
        let (_, child) = self.binding(rule, key)?;
        self.set_value(&child, key, uri)
    }

    /// The entries of `rule` and of its binding `key`.
    fn binding(&self, rule: &str, key: &str) -> Result<(Entry, Entry)> {
        let rule_entry = self.rule(rule)?;
        let wanted = Value::String(key.to_string());
        let child = self
            .children(&rule_entry, rule)?
            .entries
            .into_iter()
            .find(|e| e.key.as_ref() == Some(&wanted))
            .with_context(|| format!("cannot find `{key}` of rule `{rule}` line by line"))?;
        Ok((rule_entry, child))
    }

    fn set_value(&mut self, child: &Entry, key: &str, uri: &str) -> Result<()> {
        let (indent, key_text) = self.key_text(child, key)?;
        let comment = if child.start == child.last {
            beside_comment(&self.lines[child.start].text[indent..])
        } else {
            ""
        };
        let line = format!("{}{key_text}: {}{comment}", " ".repeat(indent), token(uri)?);
        self.replace(child.start, child.last, vec![line]);
        Ok(())
    }

    fn rule(&self, rule: &str) -> Result<Entry> {
        let wanted = Value::String(rule.to_string());
        self.level(0, self.lines.len())?
            .and_then(|top| {
                top.entries
                    .into_iter()
                    .find(|e| e.key.as_ref() == Some(&wanted))
            })
            .with_context(|| format!("cannot find rule `{rule}` line by line"))
    }

    /// The rule's bindings, written as an indented block under its key line.
    fn children(&self, rule_entry: &Entry, rule: &str) -> Result<Level> {
        let line = self.lines[rule_entry.start].text.trim_start();
        // § one qp8tmdhae.f@yh4fe
        if !matches!(single_entry(line), Some((_, Value::Null))) {
            bail!(
                "rule `{rule}` holds its bindings on its key line (flow style), which Lade cannot edit line by line"
            );
        }
        self.level(rule_entry.start + 1, rule_entry.last + 1)?
            .with_context(|| format!("rule `{rule}` has no binding lines under its key"))
    }

    /// The entries whose key lines share the indent of the first content line in
    /// `from..to`; lines indented deeper belong to the entry above them.
    fn level(&self, from: usize, to: usize) -> Result<Option<Level>> {
        let Some(indent) = (from..to)
            .find(|&i| is_content(&self.lines[i].text))
            .map(|i| indent_of(&self.lines[i].text))
        else {
            return Ok(None);
        };
        let mut entries: Vec<Entry> = Vec::new();
        for i in from..to {
            let text = &self.lines[i].text;
            if !is_content(text) {
                continue;
            }
            let at = indent_of(text);
            if at < indent {
                bail!(
                    "line {} is indented less than the lines above it",
                    i + 1 + usize::from(self.bom)
                );
            }
            if at == indent {
                entries.push(Entry {
                    key: None,
                    start: i,
                    last: i,
                });
            } else if let Some(entry) = entries.last_mut() {
                entry.last = i;
            }
        }
        for entry in &mut entries {
            entry.key = self.key_of(entry, indent);
        }
        Ok(Some(Level { indent, entries }))
    }

    fn key_of(&self, entry: &Entry, indent: usize) -> Option<Value> {
        if let Some((key, _)) = single_entry(&self.lines[entry.start].text[indent..]) {
            return Some(key);
        }
        let block: Vec<&str> = (entry.start..=entry.last)
            .map(|i| self.lines[i].text.get(indent..).unwrap_or(""))
            .collect();
        single_entry(&block.join("\n")).map(|(key, _)| key)
    }

    /// The entry's indent and its key as the file spells it, quotes included.
    fn key_text(&self, entry: &Entry, key: &str) -> Result<(usize, String)> {
        let text = &self.lines[entry.start].text;
        let indent = indent_of(text);
        let line = &text[indent..];
        let wanted = Value::String(key.to_string());
        for (at, _) in line.match_indices(':') {
            let after = line[at + 1..].chars().next();
            if !matches!(after, None | Some(' ' | '\t')) {
                continue;
            }
            if let Some((found, Value::Null)) = single_entry(&line[..=at])
                && found == wanted
            {
                return Ok((indent, line[..at].to_string()));
            }
        }
        bail!(
            "cannot read the key `{key}` on line {}",
            entry.start + 1 + usize::from(self.bom)
        )
    }

    /// The indent of a rule's bindings below its key, from the file's first block rule.
    fn child_step(&self) -> Result<usize> {
        let Some(top) = self.level(0, self.lines.len())? else {
            return Ok(2);
        };
        for entry in &top.entries {
            if entry.last == entry.start {
                continue;
            }
            if let Some(children) = self.level(entry.start + 1, entry.last + 1)? {
                return Ok(children.indent - top.indent);
            }
        }
        Ok(2)
    }

    fn replace(&mut self, start: usize, last: usize, new: Vec<String>) {
        let end = self.lines[last].end.clone();
        self.lines.drain(start..=last);
        let count = new.len();
        for (offset, text) in new.into_iter().enumerate() {
            let end = if offset + 1 == count {
                end.clone()
            } else {
                self.eol.clone()
            };
            self.lines.insert(start + offset, Line { text, end });
        }
    }

    /// Inserts `new` before line `at`, ending the line above it when it had no end.
    fn insert(&mut self, at: usize, new: Vec<String>) {
        let mut last_end = self.eol.clone();
        if at > 0 && self.lines[at - 1].end.is_empty() {
            self.lines[at - 1].end = self.eol.clone();
            if at == self.lines.len() {
                last_end = String::new();
            }
        }
        let count = new.len();
        for (offset, text) in new.into_iter().enumerate() {
            let end = if offset + 1 == count {
                last_end.clone()
            } else {
                self.eol.clone()
            };
            self.lines.insert(at + offset, Line { text, end });
        }
    }
}

fn is_empty_collection(value: &Value) -> bool {
    match value {
        Value::Mapping(map) => map.is_empty(),
        Value::Sequence(seq) => seq.is_empty(),
        _ => false,
    }
}

fn is_content(text: &str) -> bool {
    let trimmed = text.trim();
    !trimmed.is_empty() && !trimmed.starts_with('#')
}

fn indent_of(text: &str) -> usize {
    text.len() - text.trim_start_matches(' ').len()
}

fn single_entry(src: &str) -> Option<(Value, Value)> {
    let Ok(Value::Mapping(map)) = serde_yaml::from_str::<Value>(src) else {
        return None;
    };
    let mut entries = map.into_iter();
    match (entries.next(), entries.next()) {
        (Some(entry), None) => Some(entry),
        _ => None,
    }
}

/// The trailing ` # ...` of a one-line entry: the first `#` after a blank whose
/// removal leaves the entry as YAML reads the whole line.
fn beside_comment(line: &str) -> &str {
    let whole = single_entry(line);
    for (at, _) in line.match_indices('#') {
        if !line[..at].ends_with([' ', '\t']) {
            continue;
        }
        let head = line[..at].trim_end();
        if single_entry(head) == whole {
            return &line[head.len()..];
        }
    }
    ""
}

/// One scalar as YAML writes it on one line, quoted when it must be.
fn token(value: &str) -> Result<String> {
    let rendered = serde_yaml::to_string(&Value::String(value.to_string()))?;
    let rendered = rendered.strip_suffix('\n').unwrap_or(&rendered);
    if rendered.contains('\n') {
        bail!("`{value}` does not fit on one yaml line");
    }
    Ok(rendered.to_string())
}

/// The rule keys of `root` whose mapping binds `key` to a value `pick` accepts.
pub(super) fn rules_binding(root: &Value, key: &str, pick: impl Fn(&Value) -> bool) -> Vec<String> {
    let wanted = Value::String(key.to_string());
    root.as_mapping()
        .map(Mapping::iter)
        .into_iter()
        .flatten()
        .filter(|(_, entry)| {
            entry
                .as_mapping()
                .and_then(|map| map.get(&wanted))
                .is_some_and(&pick)
        })
        .filter_map(|(rule, _)| rule.as_str().map(str::to_string))
        .collect()
}
