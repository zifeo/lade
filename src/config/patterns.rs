use anyhow::{Context, Result};
use fancy_regex::Regex as FancyRegex;
use regex::{Error as RegexError, Regex, RegexSet};

/// Caps VM work on lookaround keys so a hook cannot sit on a command line.
const BACKTRACK_LIMIT: usize = 100_000;

pub(crate) struct CompiledPatterns {
    regex_set: RegexSet,
    set_rule: Vec<usize>,
    fancy: Vec<(usize, FancyRegex)>,
}

impl CompiledPatterns {
    pub(crate) fn compile(patterns: &[String]) -> Result<Self> {
        match RegexSet::new(patterns) {
            Ok(regex_set) => Ok(Self {
                regex_set,
                set_rule: (0..patterns.len()).collect(),
                fancy: Vec::new(),
            }),
            Err(err) if needs_fancy(&err) => Self::split(patterns),
            Err(err) => Err(err.into()),
        }
    }

    fn split(patterns: &[String]) -> Result<Self> {
        let mut easy = Vec::new();
        let mut set_rule = Vec::new();
        let mut fancy = Vec::new();
        for (index, pattern) in patterns.iter().enumerate() {
            match Regex::new(pattern) {
                Ok(_) => {
                    easy.push(pattern.as_str());
                    set_rule.push(index);
                }
                Err(err) if needs_fancy(&err) => {
                    let compiled = fancy_regex::RegexBuilder::new(pattern)
                        .backtrack_limit(BACKTRACK_LIMIT)
                        .build()
                        .with_context(|| format!("failed to compile command regex '{pattern}'"))?;
                    fancy.push((index, compiled));
                }
                Err(err) => return Err(err.into()),
            }
        }
        Ok(Self {
            regex_set: RegexSet::new(&easy)?,
            set_rule,
            fancy,
        })
    }

    pub(crate) fn matching_indices(&self, command: &str) -> Vec<usize> {
        if self.fancy.is_empty() {
            return self.regex_set.matches(command).into_iter().collect();
        }
        let rule_count = self
            .set_rule
            .iter()
            .copied()
            .chain(self.fancy.iter().map(|(index, _)| *index))
            .max()
            .map(|index| index + 1)
            .unwrap_or(0);
        let mut hit = vec![false; rule_count];
        for set_i in self.regex_set.matches(command) {
            hit[self.set_rule[set_i]] = true;
        }
        for (rule_i, compiled) in &self.fancy {
            if compiled.is_match(command).unwrap_or(false) {
                hit[*rule_i] = true;
            }
        }
        hit.into_iter()
            .enumerate()
            .filter_map(|(index, matched)| matched.then_some(index))
            .collect()
    }
}

fn needs_fancy(err: &RegexError) -> bool {
    let msg = err.to_string();
    msg.contains("look-around") || msg.contains("backreferences are not supported")
}
