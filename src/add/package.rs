use anyhow::{Context, Result, bail};

use crate::message_box::MessageBox;

use super::ask;
use super::cli::{family_program, login_stop, pick_from_lines};
use super::require_or_ask;

pub fn package_uri(query: Option<&str>, tty: bool) -> Result<String> {
    if let Some(query) = query {
        if query.contains("://") {
            return Ok(query.to_string());
        }
        if tty && (query == "apm" || query.starts_with("apm/")) {
            let q = query.strip_prefix("apm/").unwrap_or("");
            return search_registry("apm", q);
        }
        if tty && (query == "skills" || query.starts_with("skills/")) {
            let q = query.strip_prefix("skills/").unwrap_or("");
            return search_registry("skills", q);
        }
        if tty {
            return search_mise(query);
        }
        bail!("pass --uri mise://<backend>/<package>@<version> (got `{query}`)");
    }
    if tty {
        return require_or_ask(None, "URI (mise://…): ", true);
    }
    bail!("pass --uri")
}

fn search_registry(cli: &str, query: &str) -> Result<String> {
    let query = if query.is_empty() {
        ask(&format!("{cli} package (owner/repo): "))?
    } else {
        query.to_string()
    };
    if query.is_empty() {
        bail!("a package ref is required");
    }
    if query.contains("://") {
        return Ok(query);
    }
    let args = ["search", query.as_str()];
    match std::process::Command::new(family_program(cli)?)
        .args(args)
        .output()
    {
        Ok(output) if output.status.success() => {
            let lines: Vec<String> = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .take(20)
                .collect();
            if !lines.is_empty() {
                let picked = pick_from_lines(&format!("{cli} search `{query}`"), &lines)?;
                let reference = picked.split_whitespace().next().unwrap_or(&picked);
                return Ok(format!("{cli}://{reference}"));
            }
        }
        Ok(_) => return login_stop(cli),
        Err(_) => {
            MessageBox::new()
                .warning()
                .line(format!("`{cli}` is missing. Run `lade setup`."))
                .print_stderr();
            bail!("{cli} CLI is missing");
        }
    }
    Ok(format!("{cli}://{query}"))
}

fn search_mise(query: &str) -> Result<String> {
    let hits = mise_search(query)?;
    if hits.is_empty() {
        bail!("mise search found nothing for `{query}`. Pass --uri if you know the pin.");
    }
    let mut mb = MessageBox::new()
        .info()
        .line(format!("mise search `{query}`"));
    for (i, (name, spec)) in hits.iter().enumerate() {
        mb = mb.line(format!("  {}. {name}  {spec}", i + 1));
    }
    mb.print_stderr();
    let chosen = if hits.len() == 1 {
        let answer = ask(&format!("Use {}? [Y/n] ", hits[0].1))?;
        if answer.eq_ignore_ascii_case("n") || answer.eq_ignore_ascii_case("no") {
            bail!("not added");
        }
        hits[0].clone()
    } else {
        let answer = ask("Which (number): ")?;
        let index: usize = answer.parse().context("pick a number from the list")?;
        hits.get(index.saturating_sub(1))
            .cloned()
            .context("pick a number from the list")?
    };
    crate::mise::pin_exact(&format!("mise://{}@latest", chosen.1.replace(':', "/")))
}

fn mise_search(query: &str) -> Result<Vec<(String, String)>> {
    let output = std::process::Command::new(crate::mise::mise_program())
        .args(["search", query])
        .output()
        .context("could not run `mise search`. Run `lade setup` or pass --uri")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("mise search failed: {stderr}");
    }
    Ok(parse_mise_search(&String::from_utf8_lossy(&output.stdout)))
}

fn parse_mise_search(stdout: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        let Some(spec) = parts.iter().rev().find(|part| part.contains(':')) else {
            continue;
        };
        if spec.starts_with("http") {
            continue;
        }
        let name = parts
            .iter()
            .find(|part| !part.contains(':'))
            .copied()
            .unwrap_or(spec.split(':').next_back().unwrap_or(spec));
        if !out.iter().any(|(_, existing)| existing == spec) {
            out.push((name.to_string(), (*spec).to_string()));
        }
    }
    out
}

pub fn key_from_query_or_uri(query: Option<&str>, uri: Option<&str>) -> Option<String> {
    if let Some(query) = query.filter(|s| !s.contains("://") && !s.is_empty()) {
        return Some(query.to_string());
    }
    uri.and_then(short_name_from_uri)
}

fn short_name_from_uri(uri: &str) -> Option<String> {
    if let Ok(spec) = crate::mise::parse_spec(uri) {
        return Some(spec.short_name().to_string());
    }
    let rest = uri.split_once("://")?.1;
    let last = rest.split(['/', '?', '@']).rfind(|part| !part.is_empty())?;
    Some(last.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_name_from_mise_uri() {
        assert_eq!(
            short_name_from_uri("mise://aqua/jqlang/jq@1.7.1").as_deref(),
            Some("jq")
        );
    }

    #[test]
    fn parse_mise_search_lines() {
        let hits = parse_mise_search("jq  aqua:jqlang/jq\nnode  core:node\n");
        assert_eq!(
            hits,
            vec![
                ("jq".to_string(), "aqua:jqlang/jq".to_string()),
                ("node".to_string(), "core:node".to_string()),
            ]
        );
    }
}
