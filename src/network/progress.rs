use std::time::Instant;

#[derive(Debug, Clone)]
pub enum ProviderProgressKind {
    Connecting,
    Connected,
    Failed,
}

#[derive(Debug, Clone)]
pub struct ProviderProgressEvent {
    pub id: String,
    pub display: String,
    pub kind: ProviderProgressKind,
}

pub fn format_timing(display: &str, started: Instant) -> String {
    format_elapsed_ms(display, started.elapsed().as_millis())
}

pub fn format_elapsed_ms(display: &str, ms: u128) -> String {
    format!("{display} {ms} ms")
}
