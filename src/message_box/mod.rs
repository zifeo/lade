use owo_colors::{OwoColorize, Style};
use std::io::{IsTerminal, stderr};

mod terminal;
#[cfg(test)]
mod tests;

use terminal::*;

#[derive(Debug, Clone)]
enum Entry {
    Line(String),
    Paragraph(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tone {
    #[default]
    Info,
    Warning,
    Error,
}

/// A bordered stderr message box with an embedded title in the top border.
#[derive(Debug, Clone, Default)]
pub struct MessageBox {
    width: usize,
    entries: Vec<Entry>,
    tone: Tone,
}

impl MessageBox {
    pub fn new() -> Self {
        Self {
            width: detect_width(),
            entries: Vec::new(),
            tone: Tone::Info,
        }
    }

    pub fn info(mut self) -> Self {
        self.tone = Tone::Info;
        self
    }

    pub fn warning(mut self) -> Self {
        self.tone = Tone::Warning;
        self
    }

    pub fn error(mut self) -> Self {
        self.tone = Tone::Error;
        self
    }

    pub fn line(mut self, text: impl Into<String>) -> Self {
        self.entries.push(Entry::Line(text.into()));
        self
    }

    pub fn paragraph(mut self, text: impl Into<String>) -> Self {
        self.entries.push(Entry::Paragraph(text.into()));
        self
    }

    pub fn paragraphs<I, S>(mut self, texts: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for text in texts {
            self.entries.push(Entry::Paragraph(text.into()));
        }
        self
    }

    /// Render the entries as bare stderr lines: no border, no colour. For
    /// passive confirmations like `Lade loaded` where a full box adds visual
    /// fatigue and there is nothing to act on.
    pub fn print_plain_stderr(&self) {
        for entry in &self.entries {
            match entry {
                Entry::Line(text) => eprintln!("{text}"),
                Entry::Paragraph(text) => {
                    for line in textwrap::wrap(text.trim(), self.width) {
                        eprintln!("{line}");
                    }
                }
            }
        }
    }

    pub fn print_stderr(&self) {
        // Borders only make sense on a real terminal. In non-interactive output
        // (agents, pipes) the box does not fit and just adds noise, so fall back
        // to bare lines.
        if !stderr().is_terminal() {
            self.print_plain_stderr();
            return;
        }
        let inner = self.width - 2; // borders
        let content = inner - 6; // 3-space gutter on each side
        let colored = colors_enabled();
        let style = tone_style(self.tone, colored);

        let label = self.tone.label();
        let label_part = format!(" {label} ");
        let dash_count = inner.saturating_sub(label_part.len());
        let top = format!("╭{}{}╮", label_part, "─".repeat(dash_count));
        print_styled(&top, style, colored);

        let blank = format!("│{}│", " ".repeat(inner));
        print_styled(&blank, style, colored);

        for entry in &self.entries {
            let text = match entry {
                Entry::Line(text) => text.as_str(),
                Entry::Paragraph(text) => text.trim(),
            };
            // An empty entry wraps to nothing, so emit one blank line for it.
            let wrapped = textwrap::wrap(text, content);
            let lines: Vec<_> = if wrapped.is_empty() {
                vec![std::borrow::Cow::from("")]
            } else {
                wrapped
            };
            for line in lines {
                let padded = format!(
                    "│   {}{}   │",
                    line,
                    " ".repeat(content.saturating_sub(textwrap::core::display_width(&line)))
                );
                print_styled(&padded, style, colored);
            }
        }

        print_styled(&blank, style, colored);

        let bottom = format!("╰{}╯", "─".repeat(inner));
        print_styled(&bottom, style, colored);
    }
}

pub fn print_loaded_message(mut names: Vec<String>) {
    if names.is_empty() {
        return;
    }
    names.sort();
    eprintln!("Lade loaded: {}.", names.join(", "));
    eprintln!();
}

impl Tone {
    fn label(self) -> &'static str {
        match self {
            Tone::Info => "Info",
            Tone::Warning => "Warning",
            Tone::Error => "Error",
        }
    }
}

fn tone_style(tone: Tone, colored: bool) -> Style {
    if !colored {
        return Style::new();
    }
    match tone {
        Tone::Info => Style::new().blue(),
        Tone::Warning => Style::new().yellow(),
        Tone::Error => Style::new().red(),
    }
}

fn print_styled(line: &str, style: Style, colored: bool) {
    if colored {
        eprintln!("{}", line.style(style));
    } else {
        eprintln!("{line}");
    }
}
