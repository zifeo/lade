use std::collections::HashSet;
use std::net::SocketAddr;

pub(super) fn dedupe_lines(raw: &str) -> String {
    let mut seen = HashSet::new();
    raw.lines()
        .filter_map(|line| {
            let trimmed = line.trim_end();
            if trimmed.is_empty() || !seen.insert(dedupe_key(trimmed)) {
                None
            } else {
                Some(trimmed)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn dedupe_key(line: &str) -> String {
    if let Some((_, rest)) = line.split_once(" memcache.go:") {
        return format!("memcache.go:{rest}");
    }
    line.to_string()
}

/// The loopback address a provider picked: `kubectl port-forward :remote`
/// prints `Forwarding from HOST:PORT -> REMOTE`, `tsh proxy app` prints
/// `Proxying connections to APP on HOST:PORT`.
pub(super) fn printed_addr(line: &str) -> Option<SocketAddr> {
    let line = line.trim();
    let addr = match line.strip_prefix("Forwarding from ") {
        Some(rest) => rest.split_once(' ')?.0,
        None if line.starts_with("Proxying connections to ") => line.rsplit_once(" on ")?.1,
        None => return None,
    };
    addr.parse::<SocketAddr>()
        .ok()
        .filter(|addr| addr.ip().is_loopback())
}
