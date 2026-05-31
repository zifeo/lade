use std::io::{IsTerminal, stderr};

const DEFAULT_WIDTH: usize = 80;
const MIN_WIDTH: usize = 40;
const MAX_WIDTH: usize = 120;

#[derive(Debug, Clone)]
enum Entry {
    Line(String),
    Paragraph(String),
}

/// A bordered stderr message box with optional wrapped body lines (`> ` prefix).
#[derive(Debug, Clone, Default)]
pub struct MessageBox {
    width: usize,
    entries: Vec<Entry>,
}

impl MessageBox {
    pub fn new() -> Self {
        Self {
            width: detect_width(),
            entries: Vec::new(),
        }
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
        let wrap_width = self.width - 4;
        let border = "-".repeat(self.width - 2);

        eprintln!("┌{border}┐");
        for entry in &self.entries {
            match entry {
                Entry::Line(text) => print_plain_line(text, wrap_width),
                Entry::Paragraph(text) => {
                    for line in textwrap::wrap(text.trim(), wrap_width - 2) {
                        print_body_line(&line, wrap_width);
                    }
                }
            }
        }
        eprintln!("└{border}┘");
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

fn print_plain_line(text: &str, wrap_width: usize) {
    eprintln!(
        "| {} {}|",
        text,
        " ".repeat(wrap_width.saturating_sub(text.len()))
    );
}

fn print_body_line(line: &str, wrap_width: usize) {
    eprintln!(
        "| > {} {}|",
        line,
        " ".repeat(
            wrap_width
                .saturating_sub(2)
                .saturating_sub(textwrap::core::display_width(line))
        )
    );
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
            .line("Header")
            .paragraph("Body line one")
            .line("Footer")
            .print_stderr();
    }
}
