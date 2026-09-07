use std::io::{self, Write};

use anyhow::Result;

use crate::message_box::MessageBox;

pub(super) fn confirm(prompt: &str) -> Result<bool> {
    eprint!("{prompt} [y/N]: ");
    io::stderr().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    let answer = answer.trim().to_ascii_lowercase();
    Ok(answer == "y" || answer == "yes")
}

pub(super) fn report(title: &str, results: Vec<String>) {
    if results.is_empty() {
        return;
    }
    let mut mb = MessageBox::new().info().line(title);
    for result in results {
        mb = mb.line(format!("- {result}"));
    }
    mb.print_plain_stderr();
}
