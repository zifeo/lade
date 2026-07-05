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
    format!("{display} {} ms", started.elapsed().as_millis())
}
