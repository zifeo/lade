use std::path::Path;
use std::time::{Duration, Instant};

use super::{ERROR_MAX_CHARS, Incompressible, RuleReport};

pub(super) fn elapsed_ms(started: Instant) -> f64 {
    duration_ms(started.elapsed())
}

pub(super) fn duration_ms(duration: Duration) -> f64 {
    (duration.as_secs_f64() * 1_000.0 * 1_000.0).round() / 1_000.0
}

pub(super) fn format_timeout(timeout: Duration) -> String {
    if timeout.as_millis().is_multiple_of(1000) {
        format!("{}s", timeout.as_secs())
    } else {
        format!("{}ms", timeout.as_millis())
    }
}

pub(super) fn print_incompressible(inc: &Incompressible) {
    println!("incompressible");
    println!(
        "  parse: {} ({} files, {} rules)",
        format_ms(inc.parse_ms),
        inc.files,
        inc.rules
    );
    println!("  match: {}", format_ms(inc.match_ms));
}

pub(super) fn format_rule_line(rule: &RuleReport) -> String {
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

pub(super) fn short_error(err: &str) -> String {
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

pub(super) fn truncate_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let keep = max.saturating_sub(3);
    let mut out: String = text.chars().take(keep).collect();
    out.push_str("...");
    out
}

pub(super) fn format_ms(ms: f64) -> String {
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
