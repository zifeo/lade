use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::message_box::Report;

pub const FRAMES: [&str; 6] = ["⠋", "⠙", "⠸", "⠴", "⠦", "⠇"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveKind {
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone)]
pub struct LiveEvent {
    pub id: String,
    pub display: String,
    pub kind: LiveKind,
}

#[derive(Clone)]
pub struct LiveProgressSink {
    tx: Sender<LiveEvent>,
}

impl LiveProgressSink {
    pub fn send(&self, event: LiveEvent) {
        let _ = self.tx.send(event);
    }
}

pub struct LiveProgressRenderer {
    sink: LiveProgressSink,
    join: JoinHandle<()>,
}

impl LiveProgressRenderer {
    pub fn sink(&self) -> LiveProgressSink {
        self.sink.clone()
    }
}

static ACTIVE: Mutex<Option<LiveProgressSink>> = Mutex::new(None);
static CURRENT: Mutex<Option<(String, String)>> = Mutex::new(None);

pub struct Guard {
    renderer: Option<LiveProgressRenderer>,
}

impl Drop for Guard {
    fn drop(&mut self) {
        if let Ok(mut active) = ACTIVE.lock() {
            *active = None;
        }
        if let Ok(mut current) = CURRENT.lock() {
            *current = None;
        }
        stop(&mut self.renderer);
    }
}

pub fn begin(rich_tty: bool) -> Guard {
    start_guard(rich_tty, true)
}

pub fn begin_keep(rich_tty: bool) -> Guard {
    start_guard(rich_tty, false)
}

pub fn named_version(name: &str, version: &str) -> String {
    if version.is_empty() {
        name.to_string()
    } else {
        format!("{name} {version}")
    }
}

fn start_guard(rich_tty: bool, clear_on_stop: bool) -> Guard {
    let renderer = start_with(rich_tty, clear_on_stop);
    if let Ok(mut active) = ACTIVE.lock() {
        *active = Some(renderer.sink());
    }
    Guard {
        renderer: Some(renderer),
    }
}

pub fn is_active() -> bool {
    ACTIVE.lock().ok().is_some_and(|g| g.is_some())
}

pub fn running(id: impl Into<String>, display: impl Into<String>) {
    let id = id.into();
    let display = display.into();
    if let Ok(mut current) = CURRENT.lock() {
        *current = Some((id.clone(), display.clone()));
    }
    emit(id, display, LiveKind::Running);
}

pub fn note(detail: &str) {
    let trimmed = detail.trim();
    if trimmed.is_empty() {
        return;
    }
    let Some((id, base)) = CURRENT.lock().ok().and_then(|g| g.clone()) else {
        return;
    };
    emit(id, format!("{base} · {trimmed}"), LiveKind::Running);
}

pub fn done(id: impl Into<String>, display: impl Into<String>) {
    emit(id.into(), display.into(), LiveKind::Done);
}

pub fn failed(id: impl Into<String>, display: impl Into<String>) {
    emit(id.into(), display.into(), LiveKind::Failed);
}

fn emit(id: String, display: String, kind: LiveKind) {
    let Ok(active) = ACTIVE.lock() else {
        return;
    };
    let Some(sink) = active.as_ref() else {
        return;
    };
    sink.send(LiveEvent { id, display, kind });
}

fn start_with(rich_tty: bool, clear_on_stop: bool) -> LiveProgressRenderer {
    let (tx, rx) = mpsc::channel::<LiveEvent>();
    let join = std::thread::spawn(move || {
        let mut frame_idx = 0usize;
        let mut order = Vec::<String>::new();
        let mut states = HashMap::<String, (String, LiveKind)>::new();
        let mut drawn_lines = 0usize;
        let mut announced = HashMap::<String, bool>::new();

        loop {
            match rx.recv_timeout(Duration::from_millis(120)) {
                Ok(event) => {
                    let first = !states.contains_key(&event.id);
                    if first {
                        order.push(event.id.clone());
                    }
                    if !rich_tty && event.kind == LiveKind::Running && first {
                        Report::new().line(event.display.clone()).print();
                        announced.insert(event.id.clone(), true);
                    }
                    if !rich_tty
                        && event.kind != LiveKind::Running
                        && !announced.contains_key(&event.id)
                    {
                        Report::new().line(event.display.clone()).print();
                    }
                    states.insert(event.id, (event.display, event.kind));
                    if rich_tty {
                        drawn_lines = redraw(&order, &states, FRAMES[frame_idx], drawn_lines);
                    }
                    continue;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    if rich_tty {
                        if clear_on_stop {
                            erase(drawn_lines);
                        } else {
                            let _ = redraw(&order, &states, FRAMES[frame_idx], drawn_lines);
                        }
                    }
                    break;
                }
            }

            if rich_tty && !order.is_empty() {
                drawn_lines = redraw(&order, &states, FRAMES[frame_idx], drawn_lines);
                frame_idx = (frame_idx + 1) % FRAMES.len();
            }
        }
    });
    LiveProgressRenderer {
        sink: LiveProgressSink { tx },
        join,
    }
}

pub fn stop(renderer: &mut Option<LiveProgressRenderer>) {
    if let Some(renderer) = renderer.take() {
        drop(renderer.sink);
        let _ = renderer.join.join();
    }
}

fn erase(previous_lines: usize) {
    if previous_lines == 0 {
        return;
    }
    let mut stderr = std::io::stderr();
    let _ = write!(stderr, "\x1b[{previous_lines}A\x1b[J");
    let _ = stderr.flush();
}

pub fn redraw(
    order: &[String],
    states: &HashMap<String, (String, LiveKind)>,
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
            LiveKind::Running => frame,
            LiveKind::Done => "✔︎",
            LiveKind::Failed => "✘",
        };
        let rendered = align_timing_right(status, display, columns);
        for wrapped in wrap_to_columns(&rendered, columns) {
            let _ = writeln!(stderr, "\x1b[2K\r{wrapped}");
            current_lines += 1;
        }
    }
    if previous_lines > current_lines {
        let extra = previous_lines - current_lines;
        for _ in 0..extra {
            let _ = writeln!(stderr, "\x1b[2K\r");
        }
        let _ = write!(stderr, "\x1b[{extra}A");
    }
    let _ = stderr.flush();
    current_lines
}

pub fn align_timing_right(status: &str, display: &str, columns: Option<usize>) -> String {
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

pub fn wrap_to_columns(line: &str, columns: Option<usize>) -> Vec<String> {
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

pub fn detect_columns() -> Option<usize> {
    crate::message_box::terminal_columns().or_else(crate::message_box::columns_env)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_without_columns_is_one_line() {
        assert_eq!(
            wrap_to_columns("mise lock --upgrade", None),
            vec!["mise lock --upgrade".to_string()]
        );
    }

    #[test]
    fn align_without_timing_stays_plain() {
        assert_eq!(
            align_timing_right("✔︎", "mise lock --upgrade", Some(80)),
            "✔︎ mise lock --upgrade"
        );
    }

    #[test]
    fn named_version_is_one_space() {
        assert_eq!(named_version("mise", "2026.9.11"), "mise 2026.9.11");
        assert_eq!(named_version("mise", ""), "mise");
    }
}
