use std::io::Write;

use anyhow::{Result, bail};
use sha2::{Digest, Sha256};
use tokio::{io::AsyncBufReadExt, select, signal};

use crate::config::Config;
use crate::context::InvocationContext;
use crate::message_box::MessageBox;
use crate::shell::{LADE_ACCEPT_DISCLAIMER, LADE_DISCLAIMER_APPROVED};

pub fn disclaimer_id(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..8]) // 16 hex chars
}

pub fn is_approved(disclaimers: &[String]) -> bool {
    if std::env::var(LADE_ACCEPT_DISCLAIMER).ok().as_deref() == Some("1") {
        return true;
    }

    let approved_env = std::env::var(LADE_DISCLAIMER_APPROVED).unwrap_or_default();
    let approved_ids: std::collections::HashSet<_> = approved_env.split_whitespace().collect();

    disclaimers
        .iter()
        .all(|d| approved_ids.contains(disclaimer_id(d).as_str()))
}

pub async fn resolve_disclaimers(
    ctx: &InvocationContext,
    config: &Config,
    command: &str,
) -> Result<()> {
    let disclaimers = config.collect_disclaimers(command);
    if disclaimers.is_empty() || is_approved(&disclaimers) {
        return Ok(());
    }

    if !ctx.may_prompt() {
        MessageBox::new()
            .warning()
            .line("Disclaimer required for this command:")
            .paragraphs(disclaimers.iter().map(String::as_str))
            .print_stderr();
        bail!("lade: disclaimer required — run: lade approve");
    }

    confirm_disclaimers(&disclaimers).await
}

pub async fn confirm_disclaimers(disclaimers: &[String]) -> Result<()> {
    if disclaimers.is_empty() {
        return Ok(());
    }
    MessageBox::new()
        .info()
        .line("Are you sure you want to proceed?")
        .paragraphs(disclaimers.iter().map(String::as_str))
        .print_stderr();
    eprint!("Type \"yes\" to continue (Ctrl+C to cancel): ");
    std::io::stderr().flush()?;
    let input = read_stdin().await;
    if input.as_deref().map(str::trim) != Some("yes") {
        MessageBox::new()
            .error()
            .line("Not injecting secrets, aborting.")
            .print_stderr();
        std::process::exit(1);
    }
    Ok(())
}

async fn read_stdin() -> Option<String> {
    let mut line = String::new();
    let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
    select! {
        result = reader.read_line(&mut line) => result.ok().map(|_| line),
        _ = signal::ctrl_c() => std::process::exit(130),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_disclaimers_is_noop() {
        confirm_disclaimers(&[]).await.unwrap();
    }

    #[test]
    fn test_disclaimer_id_stable() {
        let text = "This will destroy production.";
        let id = disclaimer_id(text);
        assert_eq!(id.len(), 16);
        assert_eq!(id, disclaimer_id(text));
        assert_ne!(id, disclaimer_id("different text"));
    }

    #[test]
    fn test_is_approved_env_bypass() {
        temp_env::with_var(LADE_ACCEPT_DISCLAIMER, Some("1"), || {
            assert!(is_approved(&["any".to_string()]));
        });
    }

    #[test]
    fn test_is_approved_session() {
        let d1 = "Disclaimer 1".to_string();
        let d2 = "Disclaimer 2".to_string();
        let id1 = disclaimer_id(&d1);
        let id2 = disclaimer_id(&d2);

        temp_env::with_var(
            LADE_DISCLAIMER_APPROVED,
            Some(format!("{id1} {id2}")),
            || {
                assert!(is_approved(&[d1.clone(), d2.clone()]));
                assert!(is_approved(&[d1.clone()]));
                assert!(!is_approved(&[d1.clone(), "other".to_string()]));
            },
        );
    }
}
