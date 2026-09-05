use std::collections::BTreeSet;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use futures::stream::{FuturesUnordered, StreamExt};
use serde::Serialize;
use tokio::time::timeout;

use crate::args::BenchCommand;
use crate::config::{Config, LadeFile, LadeRule, RuleWhen, saved_user};
use crate::message_box;

/// A command that is only used to exercise `RegexSet` matching. A catch-all
/// `.` still matches it. That is fine: the incompressible cost is the scan.
const MATCH_PROBE: &str = "lade-bench-no-match";
const ERROR_MAX_CHARS: usize = 100;

#[derive(Serialize)]
struct Incompressible {
    parse_ms: f64,
    match_ms: f64,
    files: usize,
    rules: usize,
}

#[derive(Serialize)]
struct RuleReport {
    file: PathBuf,
    pattern: String,
    when: &'static str,
    hydrate_ms: f64,
    providers: Vec<String>,
    error: Option<String>,
}

#[derive(Serialize)]
struct BenchReport {
    incompressible: Incompressible,
    rules: Vec<RuleReport>,
    total_ms: f64,
    timeout_ms: f64,
}

pub async fn run(opts: BenchCommand) -> Result<()> {
    let started = Instant::now();
    let cwd = std::env::current_dir()?;
    let parse_started = Instant::now();
    let config = match LadeFile::build(cwd) {
        Ok(config) => config,
        Err(e) => {
            message_box::MessageBox::new()
                .error()
                .line("Lade could not parse a config file:")
                .line("")
                .paragraph(e.to_string())
                .line("")
                .line("Hint: check the file format.")
                .print_stderr();
            std::process::exit(crate::exit_codes::FAILURE);
        }
    };
    let parse_ms = elapsed_ms(parse_started);

    let match_started = Instant::now();
    let _ = config.collect(MATCH_PROBE);
    let match_ms = elapsed_ms(match_started);

    let files = config
        .rule_entries()
        .map(|(path, _, _)| path.clone())
        .collect::<BTreeSet<_>>()
        .len();
    let entries: Vec<(PathBuf, String, LadeRule)> = config
        .rule_entries()
        .map(|(path, pattern, rule)| (path.clone(), pattern.to_string(), rule.clone()))
        .collect();
    let incompressible = Incompressible {
        parse_ms,
        match_ms,
        files,
        rules: entries.len(),
    };

    if !opts.json {
        print_incompressible(&incompressible);
        println!("variable");
        if entries.is_empty() {
            println!("  (no rules)");
            println!("total: {}", format_ms(elapsed_ms(started)));
            return Ok(());
        }
        io::stdout().flush()?;
    }

    let config = Arc::new(config);
    let saved = Arc::new(saved_user().await?);
    let mut running = FuturesUnordered::new();
    for (path, pattern, rule) in entries {
        let config = Arc::clone(&config);
        let saved = Arc::clone(&saved);
        let hydrate_timeout = opts.timeout;
        running.push(async move {
            time_rule(&config, &path, &pattern, &rule, &saved, hydrate_timeout).await
        });
    }

    let mut rules = Vec::new();
    while let Some(report) = running.next().await {
        if !opts.json {
            println!("{}", format_rule_line(&report));
            io::stdout().flush()?;
        }
        rules.push(report);
    }

    if opts.json {
        rules.sort_by(|a, b| {
            a.hydrate_ms
                .total_cmp(&b.hydrate_ms)
                .then_with(|| a.pattern.cmp(&b.pattern))
                .then_with(|| a.file.cmp(&b.file))
        });
        let report = BenchReport {
            incompressible,
            rules,
            total_ms: elapsed_ms(started),
            timeout_ms: duration_ms(opts.timeout),
        };
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("total: {}", format_ms(elapsed_ms(started)));
    }
    Ok(())
}

async fn time_rule(
    config: &Config,
    path: &Path,
    pattern: &str,
    rule: &LadeRule,
    saved_user: &Option<String>,
    hydrate_timeout: Duration,
) -> RuleReport {
    let providers = provider_kinds(path, rule, saved_user);
    let started = Instant::now();
    let error = match timeout(
        hydrate_timeout,
        config.hydrate_rules(&[(path.to_path_buf(), rule.clone())], saved_user),
    )
    .await
    {
        Ok(Ok(_)) => None,
        Ok(Err(e)) => Some(short_error(&e.to_string())),
        Err(_) => Some(format!("timeout {}", format_timeout(hydrate_timeout))),
    };
    RuleReport {
        file: rule_file(path),
        pattern: pattern.to_string(),
        when: when_label(rule),
        hydrate_ms: elapsed_ms(started),
        providers,
        error,
    }
}

fn provider_kinds(path: &Path, rule: &LadeRule, saved_user: &Option<String>) -> Vec<String> {
    let pair = vec![(path.to_path_buf(), rule.clone())];
    let mut kinds = BTreeSet::new();
    if let Ok(plan) = Config::secret_sources_from_rules(&pair, saved_user) {
        for source in plan.sources.values() {
            kinds.insert(provider_kind(source));
        }
    }
    for binding in Config::network_bindings_from_rules(&pair, saved_user) {
        kinds.insert(provider_kind(&binding.uri));
    }
    kinds.into_iter().collect()
}

fn provider_kind(source: &str) -> String {
    match source.split_once("://").map(|(scheme, _)| scheme) {
        Some("op") => "1password".to_string(),
        Some("sh" | "bash" | "zsh" | "fish") => "sh".to_string(),
        Some(scheme) => scheme.to_string(),
        None => "raw".to_string(),
    }
}

fn rule_file(dir: &Path) -> PathBuf {
    let yaml = dir.join("lade.yaml");
    if yaml.exists() {
        yaml
    } else {
        dir.join("lade.yml")
    }
}

fn when_label(rule: &LadeRule) -> &'static str {
    match rule
        .config
        .as_ref()
        .map(|config| config.when)
        .unwrap_or_default()
    {
        RuleWhen::Always => "always",
        RuleWhen::Human => "human",
        RuleWhen::Agent => "agent",
    }
}

fn elapsed_ms(started: Instant) -> f64 {
    duration_ms(started.elapsed())
}

fn duration_ms(duration: Duration) -> f64 {
    (duration.as_secs_f64() * 1_000.0 * 1_000.0).round() / 1_000.0
}

fn format_timeout(timeout: Duration) -> String {
    if timeout.as_millis().is_multiple_of(1000) {
        format!("{}s", timeout.as_secs())
    } else {
        format!("{}ms", timeout.as_millis())
    }
}

fn print_incompressible(inc: &Incompressible) {
    println!("incompressible");
    println!(
        "  parse: {} ({} files, {} rules)",
        format_ms(inc.parse_ms),
        inc.files,
        inc.rules
    );
    println!("  match: {}", format_ms(inc.match_ms));
}

fn format_rule_line(rule: &RuleReport) -> String {
    let providers = if rule.providers.is_empty() {
        "-".to_string()
    } else {
        rule.providers.join(",")
    };
    let line = format!(
        "  {}  {}  {}  {}  {providers}",
        display_path(&rule.file),
        rule.pattern,
        rule.when,
        format_ms(rule.hydrate_ms),
    );
    match &rule.error {
        Some(err) => format!("{line}\n    error  {err}"),
        None => line,
    }
}

fn short_error(err: &str) -> String {
    let lines: Vec<&str> = err
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    let picked = lines
        .iter()
        .copied()
        .find(|line| {
            let lower = line.to_ascii_lowercase();
            lower.starts_with("message:")
                || lower.contains("connection refused")
                || lower.contains("not found")
                || lower.contains("cannot parse")
                || lower.contains("timed out")
                || lower.contains("timeout")
        })
        .or_else(|| lines.first().copied())
        .unwrap_or(err);
    let picked = picked
        .strip_prefix("Message:")
        .map(str::trim)
        .unwrap_or(picked);
    truncate_chars(picked, ERROR_MAX_CHARS)
}

fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(3);
    let mut out: String = text.chars().take(keep).collect();
    out.push_str("...");
    out
}

fn format_ms(ms: f64) -> String {
    format!("{ms:.3} ms")
}

fn display_path(path: &Path) -> String {
    if let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf())
        && let Ok(stripped) = path.strip_prefix(&home)
    {
        if stripped.as_os_str().is_empty() {
            return "~".to_string();
        }
        return format!("~/{}", stripped.display());
    }
    path.display().to_string()
}

#[cfg(test)]
mod tests {
    use super::{RuleReport, elapsed_ms, format_rule_line, short_error, truncate_chars};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    #[test]
    fn elapsed_ms_rounds_to_micros() {
        let started = Instant::now() - Duration::from_micros(1234);
        let ms = elapsed_ms(started);
        assert!(ms >= 1.234);
        assert!(ms < 50.0);
    }

    #[test]
    fn short_error_prefers_message_line() {
        let err = "Infisical error: EOF\nMessage: Project with ID 'abc' not found\nRequest: GET x";
        assert_eq!(short_error(err), "Project with ID 'abc' not found");
    }

    #[test]
    fn short_error_prefers_connection_refused() {
        let err = "Vault error: EOF\nGet \"https://127.0.0.1:8200\": connection refused";
        assert!(short_error(err).contains("connection refused"));
        assert!(!short_error(err).contains('\n'));
    }

    #[test]
    fn truncate_chars_adds_ellipsis() {
        assert_eq!(truncate_chars("abcd", 4), "abcd");
        assert_eq!(truncate_chars("abcdefghij", 7), "abcd...");
    }

    #[test]
    fn error_sits_on_the_next_line() {
        let line = format_rule_line(&RuleReport {
            file: PathBuf::from("/tmp/lade.yml"),
            pattern: "^echo g".into(),
            when: "always",
            hydrate_ms: 115.335,
            providers: vec!["passbolt".into()],
            error: Some("logging in: connection refused".into()),
        });
        let lines: Vec<&str> = line.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("passbolt"));
        assert!(!lines[0].contains("error"));
        assert_eq!(lines[1], "    error  logging in: connection refused");
    }
}
