use anyhow::{Context, Result, bail};

use crate::message_box::MessageBox;
use lade_sdk::compat::spec_for_bin;
use std::collections::HashMap;

use super::ask;

pub fn family_program(bin: &str) -> Result<std::path::PathBuf> {
    crate::mise::locked_cli_bin(bin).ok_or_else(|| {
        MessageBox::new()
            .warning()
            .line(format!("`{bin}` is missing."))
            .line("Run `lade setup` to lock it for this repo.")
            .line("Homebrew or another PATH binary is not used.")
            .print_stderr();
        anyhow::anyhow!("{bin} CLI is missing")
    })
}

pub fn login_stop<T>(cli: &str) -> Result<T> {
    MessageBox::new()
        .warning()
        .line(format!("`{cli}` needs an authenticated session."))
        .line("Lade does not pick the login command.")
        .line(format!("See {}", cli_docs(cli)))
        .line("Then `lade add` again.")
        .print_stderr();
    bail!("not logged in to {cli}")
}

fn cli_docs(cli: &str) -> &'static str {
    spec_for_bin(cli)
        .and_then(|spec| spec.docs)
        .unwrap_or("that CLI's documentation")
}

pub fn locked_path_env(bin: &str) -> Result<HashMap<String, String>> {
    let path = family_program(bin)?;
    let dir = path.parent().context("bin has no parent")?;
    let rest = std::env::var("PATH").unwrap_or_default();
    Ok(HashMap::from([(
        "PATH".to_string(),
        format!("{}:{rest}", dir.display()),
    )]))
}

pub fn pick_from_lines(title: &str, lines: &[String]) -> Result<String> {
    if lines.is_empty() {
        bail!("no {title}");
    }
    let mut mb = MessageBox::new().info().line(title);
    for (i, line) in lines.iter().take(20).enumerate() {
        mb = mb.line(format!("  {}. {line}", i + 1));
    }
    mb.print_stderr();
    let answer = ask("Which (number): ")?;
    let index: usize = answer.parse().context("pick a number from the list")?;
    lines
        .get(index.saturating_sub(1))
        .cloned()
        .context("pick a number from the list")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_docs_are_generic_product_pages() {
        assert!(cli_docs("op").starts_with("https://"));
        assert!(cli_docs("kubectl").starts_with("https://"));
        assert_eq!(cli_docs("unknown-tool"), "that CLI's documentation");
        assert!(!cli_docs("op").contains("signin"));
        assert!(!cli_docs("aws").contains("sso login"));
    }
}
