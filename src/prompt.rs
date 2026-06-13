use std::io::Write;

use anyhow::Result;
use sha2::{Digest, Sha256};
use tokio::{io::AsyncBufReadExt, select, signal};

use crate::agent;
use crate::config::Config;
use crate::context::InvocationContext;
use crate::message_box::MessageBox;
use crate::shell::{LADE_APPROVE, LADE_DISCLAIMER_APPROVED};

/// Marker error: a disclaimer-protected command was invoked without approval,
/// so secrets were withheld (fail-closed). The user-facing message has already
/// been printed; `main` maps this to [`crate::exit_codes::DISCLAIMER_WITHHELD`].
#[derive(Debug)]
pub struct DisclaimerWithheld;

impl std::fmt::Display for DisclaimerWithheld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "disclaimer required")
    }
}

impl std::error::Error for DisclaimerWithheld {}

/// Hex length of the per-command approval code shown in disclaimers.
const APPROVAL_CODE_LEN: usize = 5;
/// Seconds a given approval code stays valid. The code is bound to the command
/// and the current time window, so a fixed `LADE_APPROVE=1` reflex can no longer
/// bypass a disclaimer: each approval is a deliberate, freshly copied value.
const APPROVAL_WINDOW_SECS: u64 = 300;

pub fn disclaimer_id(text: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(text.as_bytes());
    let result = hasher.finalize();
    hex::encode(&result[..8]) // 16 hex chars
}

fn current_window() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / APPROVAL_WINDOW_SECS)
        .unwrap_or(0)
}

fn code_for_window(command: &str, window: u64) -> String {
    let mut hasher = Sha256::new();
    hasher.update(command.as_bytes());
    hasher.update(b"\n");
    hasher.update(window.to_le_bytes());
    hex::encode(hasher.finalize())[..APPROVAL_CODE_LEN].to_string()
}

/// The approval code to display for `command` right now.
pub fn approval_code(command: &str) -> String {
    code_for_window(command, current_window())
}

/// Whether `candidate` is a valid approval code for `command`. The current and
/// previous window are accepted so a code copied near a boundary still works.
pub fn verify_code(command: &str, candidate: &str) -> bool {
    let w = current_window();
    candidate == code_for_window(command, w)
        || candidate == code_for_window(command, w.saturating_sub(1))
}

pub fn is_approved(command: &str, disclaimers: &[String]) -> bool {
    if let Ok(val) = std::env::var(LADE_APPROVE)
        && verify_code(command, &val)
    {
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
    if disclaimers.is_empty() || is_approved(command, &disclaimers) {
        return Ok(());
    }

    if !ctx.is_interactive() {
        let code = approval_code(command);
        let mut mb = MessageBox::new()
            .warning()
            .line("Disclaimer required to uncover the secrets for this command:")
            .paragraphs(disclaimers.iter().map(|d| format!("> {d}")))
            .line("");
        // Agents have no persisted LADE_PENDING, so `lade approve` is useless to
        // them; point them at the code prefix instead.
        mb = if agent::detect_agent().is_some() {
            mb.line(format!(
                "Ask the user to approve, then re-run the command prefixed with LADE_APPROVE={code}."
            ))
        } else {
            mb.line(format!(
                "To approve, re-run the command prefixed with LADE_APPROVE={code}, or run `lade approve {code}`."
            ))
        };
        mb.print_stderr();
        return Err(DisclaimerWithheld.into());
    }

    confirm_disclaimers(&disclaimers).await
}

pub async fn confirm_disclaimers(disclaimers: &[String]) -> Result<()> {
    if disclaimers.is_empty() {
        return Ok(());
    }
    MessageBox::new()
        .warning()
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
        std::process::exit(crate::exit_codes::FAILURE);
    }
    Ok(())
}

async fn read_stdin() -> Option<String> {
    let mut line = String::new();
    let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
    select! {
        result = reader.read_line(&mut line) => result.ok().map(|_| line),
        _ = signal::ctrl_c() => std::process::exit(crate::exit_codes::INTERRUPTED),
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
    fn test_is_approved_with_code() {
        let cmd = "deploy prod";
        let code = approval_code(cmd);
        temp_env::with_var(LADE_APPROVE, Some(code.as_str()), || {
            assert!(is_approved(cmd, &["any".to_string()]));
        });
    }

    #[test]
    fn test_is_approved_rejects_wrong_code_and_one() {
        temp_env::with_var(LADE_APPROVE, Some("1"), || {
            assert!(!is_approved("deploy prod", &["any".to_string()]));
        });
        temp_env::with_var(LADE_APPROVE, Some("00000"), || {
            assert!(!is_approved("deploy prod", &["any".to_string()]));
        });
    }

    #[test]
    fn test_code_is_command_specific() {
        assert_eq!(approval_code("a").len(), APPROVAL_CODE_LEN);
        assert_ne!(approval_code("a"), approval_code("b"));
    }

    #[test]
    fn test_verify_code() {
        let cmd = "deploy prod";
        assert!(verify_code(cmd, &approval_code(cmd)));
        assert!(!verify_code(cmd, "00000"));
        assert!(!verify_code(cmd, "1"));
    }

    #[test]
    fn test_is_approved_session() {
        let d1 = "Disclaimer 1".to_string();
        let d2 = "Disclaimer 2".to_string();
        let id1 = disclaimer_id(&d1);
        let id2 = disclaimer_id(&d2);

        temp_env::with_vars(
            [
                (LADE_APPROVE, None),
                (
                    LADE_DISCLAIMER_APPROVED,
                    Some(format!("{id1} {id2}").as_str()),
                ),
            ],
            || {
                assert!(is_approved("cmd", &[d1.clone(), d2.clone()]));
                assert!(is_approved("cmd", std::slice::from_ref(&d1)));
                assert!(!is_approved("cmd", &[d1.clone(), "other".to_string()]));
            },
        );
    }
}
