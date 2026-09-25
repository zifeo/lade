use owo_colors::{OwoColorize, Style};
use std::io::{self, Write};

use super::terminal::colors_enabled;

#[derive(Debug, Clone)]
enum Entry {
    Heading(String),
    Line(String),
    Dim(String),
    Blank,
}

/// Unboxed command result. Bold headings, optional colour. No border.
#[derive(Debug, Clone, Default)]
pub struct Report {
    entries: Vec<Entry>,
}

impl Report {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn heading(mut self, text: impl Into<String>) -> Self {
        self.entries.push(Entry::Heading(text.into()));
        self
    }

    pub fn line(mut self, text: impl Into<String>) -> Self {
        self.entries.push(Entry::Line(text.into()));
        self
    }

    pub fn dim(mut self, text: impl Into<String>) -> Self {
        self.entries.push(Entry::Dim(text.into()));
        self
    }

    pub fn blank(mut self) -> Self {
        self.entries.push(Entry::Blank);
        self
    }

    pub fn print(self) {
        let colored = colors_enabled();
        for entry in self.entries {
            match entry {
                Entry::Heading(text) => print_styled(&text, Style::new().bold(), colored),
                Entry::Line(text) => eprintln!("{text}"),
                Entry::Dim(text) => print_styled(&text, Style::new().dimmed(), colored),
                Entry::Blank => eprintln!(),
            }
        }
        let _ = io::stderr().flush();
    }

    pub fn progress(text: impl Into<String>) {
        Self::new().line(text.into()).print();
    }
}

fn print_styled(text: &str, style: Style, colored: bool) {
    if colored {
        eprintln!("{}", text.style(style));
    } else {
        eprintln!("{text}");
    }
}
