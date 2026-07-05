#[derive(Debug, Clone)]
pub(crate) enum LocalTarget {
    FixedPort(u16),
    EnvVar(String),
}

pub(crate) type ProviderSpec = lade_sdk::network::ProviderSpec;

#[derive(Debug, Clone)]
pub(crate) struct ParsedBinding {
    pub(crate) target: LocalTarget,
    pub(crate) local_host: String,
    pub(crate) local_port: Option<u16>,
    pub(crate) spec: ProviderSpec,
    pub(crate) source_uri: String,
}

#[derive(Debug)]
pub struct AcquiredNetwork {
    pub env: std::collections::HashMap<String, String>,
    pub sources: Vec<String>,
    pub(crate) _guards: Vec<crate::network::process::RunningForward>,
}

impl AcquiredNetwork {
    pub fn empty() -> Self {
        Self {
            env: std::collections::HashMap::new(),
            sources: Vec::new(),
            _guards: Vec::new(),
        }
    }
}

#[derive(Debug)]
pub struct DetachedNetworkSession {
    pub env: std::collections::HashMap<String, String>,
    pub pids: Vec<u32>,
}

impl DetachedNetworkSession {
    pub fn empty() -> Self {
        Self {
            env: std::collections::HashMap::new(),
            pids: Vec::new(),
        }
    }
}
