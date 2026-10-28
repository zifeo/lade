use std::collections::HashMap;
use std::io::Write;
use std::sync::Mutex;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::live_progress::{self, LiveKind};
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
        let mut frame_idx = 0usize;
        let mut order = Vec::<String>::new();
        let mut states = HashMap::<String, (String, LiveKind)>::new();

        loop {
            match rx.recv_timeout(Duration::from_millis(120)) {
                Ok(event) => {
                    if !states.contains_key(&event.id) {
                        order.push(event.id.clone());
                    }
                    // An agent needs no news that a secret loaded.
                    if !rich_tty && !crate::message_box::terse() {
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
                        message_box::Report::new().line(message).print();
                    }
                    states.insert(event.id, (event.display, map_kind(event.kind)));
                    if rich_tty {
                        draw(&order, &states, live_progress::FRAMES[frame_idx]);
                    }
                    continue;
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    if rich_tty {
                        draw(&order, &states, live_progress::FRAMES[frame_idx]);
                        if let Ok(mut drawn) = DRAWN.lock() {
                            *drawn = 0;
                        }
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
                draw(&order, &states, live_progress::FRAMES[frame_idx]);
                frame_idx = (frame_idx + 1) % live_progress::FRAMES.len();
            }
        }
    });
    ProviderProgressRenderer {
        sink: ProviderProgressSink { tx },
        join,
    }
}

/// Lines the spinner has on screen. A provider lent the terminal may write
/// below them, so they are wiped first and redrawn after.
static DRAWN: Mutex<usize> = Mutex::new(0);

/// Never while a provider holds the terminal: no frame over its prompt.
fn draw(order: &[String], states: &HashMap<String, (String, LiveKind)>, frame: &str) {
    if lade_sdk::terminal_lent() {
        return;
    }
    if let Ok(mut drawn) = DRAWN.lock() {
        *drawn = live_progress::redraw(order, states, frame, *drawn);
    }
}

/// Before a provider gets the terminal: wipe the spinner, cursor back at its
/// first line, so the next frame lands where the provider's output ends.
pub(crate) fn wipe() {
    if let Ok(mut drawn) = DRAWN.lock()
        && *drawn > 0
    {
        live_progress::redraw(&[], &HashMap::new(), "", *drawn);
        *drawn = 0;
    }
}

pub fn stop_provider_progress(renderer: &mut Option<ProviderProgressRenderer>) {
    if let Some(renderer) = renderer.take() {
        drop(renderer.sink);
        let _ = renderer.join.join();
    }
}

fn map_kind(kind: ProviderProgressKind) -> LiveKind {
    match kind {
        ProviderProgressKind::Connecting => LiveKind::Running,
        ProviderProgressKind::Connected => LiveKind::Done,
        ProviderProgressKind::Failed => LiveKind::Failed,
    }
}
