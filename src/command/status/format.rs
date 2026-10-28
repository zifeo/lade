//! The status page's layout: indented sections, aligned pairs, and tables.

use std::io::IsTerminal;

use chrono::{DateTime, Local, Utc};
use owo_colors::{OwoColorize, Style};

const INDENT: &str = "  ";
const GAP: &str = "  ";

pub(super) struct Printer {
    colored: bool,
}

impl Printer {
    pub(super) fn new() -> Self {
        let colored = std::env::var_os("NO_COLOR").is_none()
            && !std::env::var("TERM").is_ok_and(|term| term.eq_ignore_ascii_case("dumb"))
            && std::io::stdout().is_terminal();
        Self { colored }
    }

    pub(super) fn styled(&self, text: &str, style: Style) -> String {
        if self.colored {
            text.style(style).to_string()
        } else {
            text.to_string()
        }
    }

    pub(super) fn heading(&self, text: &str) {
        println!("{}", self.styled(text, Style::new().bold()));
    }

    pub(super) fn line(&self, text: &str) {
        println!("{INDENT}{text}");
    }

    pub(super) fn blank(&self) {
        println!();
    }

    pub(super) fn pairs(&self, rows: &[(&str, String)]) {
        let width = rows.iter().map(|(label, _)| label.len()).max().unwrap_or(0);
        for (label, value) in rows {
            let label = self.styled(&format!("{label:<width$}"), Style::new().dimmed());
            println!("{INDENT}{label}{GAP}{value}");
        }
    }

    pub(super) fn table(&self, headers: &[&str], rows: &[Vec<String>]) {
        let header = headers.iter().map(|h| h.to_string()).collect::<Vec<_>>();
        let lines = table_lines(&header, rows);
        let (first, rest) = lines.split_first().expect("a table has a header");
        println!("{INDENT}{}", self.styled(first, Style::new().dimmed()));
        for line in rest {
            println!("{INDENT}{line}");
        }
    }
}

/// Left-aligned columns, no trailing spaces. The header is the first line.
pub(super) fn table_lines(header: &[String], rows: &[Vec<String>]) -> Vec<String> {
    let mut widths = header.iter().map(|h| h.chars().count()).collect::<Vec<_>>();
    for row in rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    std::iter::once(header)
        .chain(rows.iter().map(Vec::as_slice))
        .map(|cells| {
            let line = cells
                .iter()
                .zip(&widths)
                .map(|(cell, width)| {
                    let pad = width - cell.chars().count();
                    format!("{cell}{}", " ".repeat(pad))
                })
                .collect::<Vec<_>>()
                .join(GAP);
            line.trim_end().to_string()
        })
        .collect()
}

pub(super) fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    if n < 1024 {
        format!("{n} B")
    } else if (n as f64) < MB {
        format!("{:.1} KB", n as f64 / KB)
    } else if (n as f64) < GB {
        format!("{:.1} MB", n as f64 / MB)
    } else {
        format!("{:.1} GB", n as f64 / GB)
    }
}

pub(super) fn format_tried_at(checked_at: DateTime<Utc>, now: DateTime<Local>) -> String {
    let local = checked_at.with_timezone(&Local);
    let today = now.date_naive();
    let day = local.date_naive();
    let time = local.format("%H:%M");
    if day == today {
        format!("tried today at {time}")
    } else if day.checked_add_days(chrono::Days::new(1)) == Some(today) {
        format!("tried yesterday at {time}")
    } else {
        format!("tried {} at {time}", local.format("%d %b %Y"))
    }
}
