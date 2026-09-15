use anyhow::{Context, Result, bail};

use crate::message_box::MessageBox;

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
    match cli {
        "op" => "https://developer.1password.com/docs/cli/get-started/",
        "vault" => "https://developer.hashicorp.com/vault/docs/commands/login",
        "aws" => "https://docs.aws.amazon.com/cli/latest/userguide/cli-chap-configure.html",
        "az" => "https://learn.microsoft.com/en-us/cli/azure/authenticate-azure-cli",
        "gcloud" => "https://cloud.google.com/sdk/docs/authorizing",
        "kubectl" => "https://kubernetes.io/docs/reference/access-authn-authz/authentication/",
        "tsh" => "https://goteleport.com/docs/connect-your-client/tsh/",
        "passbolt" => "https://www.passbolt.com/docs/user-guide/cli/",
        "doppler" => "https://docs.doppler.com/docs/cli",
        "infisical" => "https://infisical.com/docs/cli/overview",
        "bw" => "https://bitwarden.com/help/cli/",
        _ => "that CLI's documentation",
    }
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

pub fn run_cli_lines(bin: &str, args: &[&str]) -> Result<Vec<String>> {
    let output = match std::process::Command::new(family_program(bin)?)
        .args(args)
        .output()
    {
        Ok(output) if output.status.success() => output,
        Ok(_) | Err(_) => return login_stop(bin),
    };
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| line.trim_start_matches("namespace/").to_string())
        .map(|line| line.split('/').next_back().unwrap_or(&line).to_string())
        .collect())
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
