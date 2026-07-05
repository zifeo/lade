use std::collections::HashMap;
use std::io::IsTerminal;
use std::io::Write;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::message_box;
use crate::network::{ProviderProgressEvent, ProviderProgressKind};

#[derive(Clone)]
pub struct ProviderProgressSink {
    tx: Sender<ProviderProgressEvent>,
}

impl ProviderProgressSink {
    pub fn send(&self, event: ProviderProgressEvent) {
        let _ = self.tx.send(event);
    }
}

pub struct ProviderProgressRenderer {
    sink: ProviderProgressSink,
    join: JoinHandle<()>,
}

impl ProviderProgressRenderer {
    pub fn sink(&self) -> ProviderProgressSink {
        self.sink.clone()
    }
}

pub fn start_provider_progress(rich_tty: bool) -> ProviderProgressRenderer {
    let (tx, rx) = mpsc::channel::<ProviderProgressEvent>();
    let join = std::thread::spawn(move || {
        let frames = ["⠋", "⠙", "⠸", "⠴", "⠦", "⠇"];
        let mut frame_idx = 0usize;
        let mut order = Vec::<String>::new();
        let mut states = HashMap::<String, (String, ProviderProgressKind)>::new();
        let mut drawn_lines = 0usize;

        loop {
            match rx.recv_timeout(Duration::from_millis(120)) {
                Ok(event) => {
                    if !states.contains_key(&event.id) {
                        order.push(event.id.clone());
                    }
                    if !rich_tty {
                        let message = match &event.kind {
                            ProviderProgressKind::Connecting => {
                                format!("Lade connecting: {}", event.display)
                            }
                            ProviderProgressKind::Connected => {
                                format!("Lade connected: {}", event.display)
                            }
                            ProviderProgressKind::Failed => {
                                format!("Lade failed: {}", event.display)
                            }
                        };
                        message_box::MessageBox::new()
                            .info()
                            .line(message)
                            .print_plain_stderr();
                    }
                    states.insert(event.id, (event.display, event.kind));
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    if rich_tty {
                        let _ = render_network_progress(
                            &order,
                            &states,
                            frames[frame_idx],
                            drawn_lines,
                        );
                        if !order.is_empty() {
                            let mut stderr = std::io::stderr();
                            let _ = writeln!(stderr);
                            let _ = stderr.flush();
                        }
                    }
                    break;
                }
            }

            if rich_tty && !order.is_empty() {
                drawn_lines =
                    render_network_progress(&order, &states, frames[frame_idx], drawn_lines);
                frame_idx = (frame_idx + 1) % frames.len();
            }
        }
    });
    ProviderProgressRenderer {
        sink: ProviderProgressSink { tx },
        join,
    }
}

pub fn stop_provider_progress(renderer: &mut Option<ProviderProgressRenderer>) {
    if let Some(renderer) = renderer.take() {
        drop(renderer.sink);
        let _ = renderer.join.join();
    }
}

fn render_network_progress(
    order: &[String],
    states: &HashMap<String, (String, ProviderProgressKind)>,
    frame: &str,
    previous_lines: usize,
) -> usize {
    let columns = detect_columns();
    let mut stderr = std::io::stderr();
    if previous_lines > 0 {
        let _ = write!(stderr, "\x1b[{previous_lines}A");
    }
    let mut current_lines = 0usize;
    for id in order {
        let Some((display, kind)) = states.get(id) else {
            continue;
        };
        let status = match kind {
            ProviderProgressKind::Connecting => frame,
            ProviderProgressKind::Connected => "✔︎",
            ProviderProgressKind::Failed => "✘",
        };
        let rendered = align_timing_right(status, display, columns);
        for wrapped in wrap_to_columns(&rendered, columns) {
            let _ = writeln!(stderr, "\x1b[2K\r{wrapped}");
            current_lines += 1;
        }
    }
    while current_lines < previous_lines {
        let _ = writeln!(stderr, "\x1b[2K\r");
        current_lines += 1;
    }
    let _ = stderr.flush();
    current_lines
}

fn align_timing_right(status: &str, display: &str, columns: Option<usize>) -> String {
    let Some(raw_width) = columns else {
        return format!("{status} {display}");
    };
    let width = raw_width.saturating_sub(1);
    if width == 0 {
        return format!("{status} {display}");
    }
    let Some((left_with_number, unit)) = display.rsplit_once(' ') else {
        return format!("{status} {display}");
    };
    if unit != "ms" {
        return format!("{status} {display}");
    }
    let Some((left_raw, number)) = left_with_number.rsplit_once(' ') else {
        return format!("{status} {display}");
    };
    if number.parse::<u128>().is_err() {
        return format!("{status} {display}");
    }
    let left = left_raw.trim_end_matches('·').trim_end();
    let right = format!("{number} ms");
    let prefix = format!("{status} {left}");
    let used = prefix.chars().count() + 1 + right.chars().count();
    if used >= width {
        return format!("{status} {display}");
    }
    let spaces = " ".repeat(width - used);
    format!("{prefix}{spaces}{right}")
}

fn wrap_to_columns(line: &str, columns: Option<usize>) -> Vec<String> {
    let Some(raw_width) = columns else {
        return vec![line.to_string()];
    };
    let width = raw_width.saturating_sub(1);
    if width == 0 {
        return vec![String::new()];
    }
    let wrapped = textwrap::wrap(line, width)
        .into_iter()
        .map(|segment| segment.into_owned())
        .collect::<Vec<_>>();
    if wrapped.is_empty() {
        return vec![String::new()];
    }
    wrapped
}

fn detect_columns() -> Option<usize> {
    #[cfg(unix)]
    {
        use std::mem::MaybeUninit;
        use std::os::unix::io::AsRawFd;

        let err = std::io::stderr();
        if err.is_terminal() {
            let mut ws = MaybeUninit::<nix::libc::winsize>::uninit();
            let ret = unsafe {
                nix::libc::ioctl(err.as_raw_fd(), nix::libc::TIOCGWINSZ, ws.as_mut_ptr())
            };
            if ret >= 0 {
                let cols = unsafe { ws.assume_init() }.ws_col as usize;
                if cols > 0 {
                    return Some(cols);
                }
            }
        }
    }
    std::env::var("COLUMNS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|cols| *cols > 0)
}
