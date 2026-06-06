use std::io::Write;

use anyhow::Result;
use chrono::TimeDelta;
use tokio::{io::AsyncBufReadExt, select, signal, time};

use crate::message_box::MessageBox;

pub async fn confirm_disclaimers(disclaimers: &[String]) -> Result<()> {
    if disclaimers.is_empty() {
        return Ok(());
    }
    MessageBox::new()
        .action()
        .line("Are you sure you want to proceed?")
        .paragraphs(disclaimers.iter().map(String::as_str))
        .print_stderr();
    eprint!("Type \"yes\" to continue (Ctrl+C to cancel): ");
    std::io::stderr().flush()?;
    let input = read_stdin(None).await;
    if input.as_deref().map(str::trim) != Some("yes") {
        eprintln!();
        eprintln!("Not injecting secrets, aborting.");
        std::process::exit(1);
    }
    Ok(())
}

pub async fn read_stdin(timeout_secs: Option<u64>) -> Option<String> {
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
        _ = signal::ctrl_c() => std::process::exit(130),
    }
}

pub async fn ask_snooze_offset() -> Option<TimeDelta> {
    eprint!("Silence for: [1]=1h  [2]=24h  [3]=7d  [Enter]=continue: ");
    std::io::stderr().flush().ok();
    let result = read_stdin(Some(5)).await;
    eprintln!();
    let snooze = match result.as_deref().map(str::trim) {
        Some("1") => Snooze::Hour,
        Some("2") => Snooze::Day,
        Some("3") => Snooze::Week,
        _ => return None,
    };
    // check fires when stored + 1d < now, so stored = now + (snooze - 1d)
    let delta = match snooze {
        Snooze::Hour => TimeDelta::try_hours(1).unwrap(),
        Snooze::Day => TimeDelta::try_days(1).unwrap(),
        Snooze::Week => TimeDelta::try_days(7).unwrap(),
    };
    Some(delta - TimeDelta::try_days(1).unwrap())
}

enum Snooze {
    Hour,
    Day,
    Week,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_disclaimers_is_noop() {
        confirm_disclaimers(&[]).await.unwrap();
    }
}
