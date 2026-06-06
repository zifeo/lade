use std::io::{IsTerminal, stderr};

use owo_colors::{OwoColorize, Style};

const DEFAULT_WIDTH: usize = 80;
const MIN_WIDTH: usize = 40;
const MAX_WIDTH: usize = 120;

#[derive(Debug, Clone)]
enum Entry {
    Line(String),
    Paragraph(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Tone {
    #[default]
    Action,
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
            tone: Tone::Action,
        }
    }

    pub fn action(mut self) -> Self {
        self.tone = Tone::Action;
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

    pub fn print_stderr(&self) {
        // inner width = total width - 2 (borders)
        let inner = self.width - 2;
        // content width = inner - 6 (3-space gutter on each side)
        let content = inner - 6;
        let colored = colors_enabled();
        let style = tone_style(self.tone, colored);

        // Top border: ╭ Title ───────╮
        let label = self.tone.label();
        // " Title " occupies label.len() + 2 spaces
        let label_part = format!(" {label} ");
        let dash_count = inner.saturating_sub(label_part.len());
        let top = format!("╭{}{}╮", label_part, "─".repeat(dash_count));
        print_styled(&top, style, colored);

        // blank padding line
        let blank = format!("│{}│", " ".repeat(inner));
        print_styled(&blank, style, colored);

        for entry in &self.entries {
            match entry {
                Entry::Line(text) => {
                    let padded = format!(
                        "│   {}{}   │",
                        text,
                        " ".repeat(content.saturating_sub(textwrap::core::display_width(text)))
                    );
                    print_styled(&padded, style, colored);
                }
                Entry::Paragraph(text) => {
                    for line in textwrap::wrap(text.trim(), content) {
                        let padded = format!(
                            "│   {}{}   │",
                            line,
                            " ".repeat(
                                content.saturating_sub(textwrap::core::display_width(&line))
                            )
                        );
                        print_styled(&padded, style, colored);
                    }
                }
            }
        }

        // blank padding line
        print_styled(&blank, style, colored);

        let bottom = format!("╰{}╯", "─".repeat(inner));
        print_styled(&bottom, style, colored);
    }
}

impl Tone {
    fn label(self) -> &'static str {
        match self {
            Tone::Action => "Action",
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
        Tone::Action => Style::new().cyan(),
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

fn detect_width() -> usize {
    clamp_width(
        terminal_columns()
            .or_else(columns_env)
            .unwrap_or(DEFAULT_WIDTH),
    )
}

fn clamp_width(width: usize) -> usize {
    width.clamp(MIN_WIDTH, MAX_WIDTH)
}

fn columns_env() -> Option<usize> {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&cols| cols > 0)
}

#[cfg(unix)]
fn terminal_columns() -> Option<usize> {
    use std::mem::MaybeUninit;
    use std::os::unix::io::AsRawFd;

    let err = stderr();
    if !err.is_terminal() {
        return None;
    }

    let mut ws = MaybeUninit::<nix::libc::winsize>::uninit();
    let ret = unsafe { nix::libc::ioctl(err.as_raw_fd(), nix::libc::TIOCGWINSZ, ws.as_mut_ptr()) };
    if ret < 0 {
        return None;
    }

    let cols = unsafe { ws.assume_init() }.ws_col as usize;
    (cols > 0).then_some(cols)
}

#[cfg(not(unix))]
fn terminal_columns() -> Option<usize> {
    if !stderr().is_terminal() {
        return None;
    }
    columns_env()
}

fn colors_enabled() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var("TERM")
        .ok()
        .is_some_and(|term| term.eq_ignore_ascii_case("dumb"))
    {
        return false;
    }
    stderr().is_terminal()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_width_bounds() {
        assert_eq!(clamp_width(10), MIN_WIDTH);
        assert_eq!(clamp_width(80), 80);
        assert_eq!(clamp_width(200), MAX_WIDTH);
    }

    #[test]
    fn columns_env_reads_variable() {
        temp_env::with_var("COLUMNS", Some("100"), || {
            assert_eq!(columns_env(), Some(100));
        });
    }

    #[test]
    fn empty_box_prints_borders_only() {
        MessageBox::new().print_stderr();
    }

    #[test]
    fn mixed_entries() {
        MessageBox::new()
            .action()
            .line("Header")
            .paragraph("Body line one")
            .line("Footer")
            .print_stderr();
    }

    #[test]
    fn warning_box() {
        MessageBox::new()
            .warning()
            .line("Something deprecated")
            .print_stderr();
    }

    #[test]
    fn error_box() {
        MessageBox::new()
            .error()
            .line("Fatal problem")
            .print_stderr();
    }
}
