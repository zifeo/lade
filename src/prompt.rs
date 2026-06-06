use std::io::{IsTerminal, Write};

use anyhow::Result;
use chrono::TimeDelta;
use tokio::{io::AsyncBufReadExt, select, signal, time};

use crate::message_box::MessageBox;

pub async fn confirm_disclaimers(disclaimers: &[String]) -> Result<()> {
    if disclaimers.is_empty() {
        return Ok(());
    }
    MessageBox::new()
        .info()
        .line("Are you sure you want to proceed?")
        .paragraphs(disclaimers.iter().map(String::as_str))
        .print_stderr();
    if !std::io::stderr().is_terminal() {
        eprintln!("Disclaimer requires an interactive shell; use `lade inject` instead.");
        std::process::exit(1);
    }
    eprint!("Type \"yes\" to continue (Ctrl+C to cancel): ");
    std::io::stderr().flush()?;
    let input = read_stdin(None, CtrlC::Exit).await;
    if input.as_deref().map(str::trim) != Some("yes") {
        eprintln!();
        eprintln!("Not injecting secrets, aborting.");
        std::process::exit(1);
    }
    Ok(())
}

// Exit: safety prompts (disclaimer) — Ctrl+C must abort without injecting secrets.
// Skip: optional nudges — Ctrl+C dismisses the prompt, same as waiting out the timeout.
enum CtrlC {
    Exit,
    Skip,
}

async fn read_stdin(timeout_secs: Option<u64>, on_ctrl_c: CtrlC) -> Option<String> {
    let mut line = String::new();
    let mut reader = tokio::io::BufReader::new(tokio::io::stdin());
    let timeout = async {
        match timeout_secs {
            Some(s) => time::sleep(time::Duration::from_secs(s)).await,
            None => std::future::pending::<()>().await,
        }
    };
    select! {
        result = reader.read_line(&mut line) => result.ok().map(|_| line),
        _ = timeout => None,
        _ = signal::ctrl_c() => match on_ctrl_c {
            CtrlC::Exit => std::process::exit(130),
            CtrlC::Skip => None,
        },
    }
}

pub enum UpgradeChoice {
    Upgrade,
    Snooze(TimeDelta),
    Continue,
}

pub async fn ask_upgrade_choice() -> UpgradeChoice {
    eprint!(
        "[Enter]=lade upgrade -y  [1]=snooze 1h  [2]=snooze 24h  [3]=snooze 7d  (continues in 5s): "
    );
    std::io::stderr().flush().ok();
    let result = read_stdin(Some(5), CtrlC::Skip).await;
    eprintln!();
    match result.as_deref().map(str::trim) {
        None => UpgradeChoice::Continue,
        Some("") => UpgradeChoice::Upgrade,
        Some("1") => UpgradeChoice::Snooze(snooze_offset(Snooze::Hour)),
        Some("2") => UpgradeChoice::Snooze(snooze_offset(Snooze::Day)),
        Some("3") => UpgradeChoice::Snooze(snooze_offset(Snooze::Week)),
        Some(_) => UpgradeChoice::Continue,
    }
}

pub async fn ask_snooze_offset() -> Option<TimeDelta> {
    eprint!("[1]=snooze 1h  [2]=snooze 24h  [3]=snooze 7d  [Enter]=continue  (continues in 5s): ");
    std::io::stderr().flush().ok();
    let result = read_stdin(Some(5), CtrlC::Skip).await;
    eprintln!();
    let snooze = match result.as_deref().map(str::trim) {
        Some("1") => Snooze::Hour,
        Some("2") => Snooze::Day,
        Some("3") => Snooze::Week,
        _ => return None,
    };
    Some(snooze_offset(snooze))
}

enum Snooze {
    Hour,
    Day,
    Week,
}

fn snooze_offset(snooze: Snooze) -> TimeDelta {
    // check fires when stored + 1d < now, so stored = now + (snooze - 1d)
    let delta = match snooze {
        Snooze::Hour => TimeDelta::try_hours(1).unwrap(),
        Snooze::Day => TimeDelta::try_days(1).unwrap(),
        Snooze::Week => TimeDelta::try_days(7).unwrap(),
    };
    delta - TimeDelta::try_days(1).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_disclaimers_is_noop() {
        confirm_disclaimers(&[]).await.unwrap();
    }
}
