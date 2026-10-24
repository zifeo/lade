use nix::unistd::{Pid, getpgid};
use std::sync::{Mutex, PoisonError};

use super::Endpoint;

/// The provider binds this port. Ready only if that child owns the listen.
pub(crate) struct Direct {
    pub(crate) host: String,
    pub(crate) port: u16,
    claim: Mutex<Option<Box<dyn Send>>>,
}

impl Direct {
    pub(crate) fn holding(outer: impl Send + 'static, host: String, port: u16) -> Self {
        Self {
            host,
            port,
            claim: Mutex::new(Some(Box::new(outer))),
        }
    }

    pub(crate) fn yield_port(&self) {
        drop(
            self.claim
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take(),
        );
    }

    pub(crate) fn endpoint(&self) -> Endpoint {
        Endpoint::Tcp {
            host: self.host.clone(),
            port: self.port,
        }
    }
}

impl std::fmt::Debug for Direct {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Direct")
            .field("host", &self.host)
            .field("port", &self.port)
            .finish()
    }
}

pub(super) const STRANGER_LISTEN: &str = "another process is listening";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PortListen {
    Vacant,
    Ours,
    Stranger,
}

pub(super) fn port_listen(port: u16, child: u32) -> PortListen {
    let pids = listen_pids(port);
    if pids.is_empty() {
        return PortListen::Vacant;
    }
    if pids.iter().any(|pid| same_group(*pid, child)) {
        PortListen::Ours
    } else {
        PortListen::Stranger
    }
}

fn same_group(pid: u32, leader: u32) -> bool {
    if pid == leader {
        return true;
    }
    let Ok(raw) = i32::try_from(pid) else {
        return false;
    };
    let Ok(leader) = i32::try_from(leader) else {
        return false;
    };
    getpgid(Some(Pid::from_raw(raw)))
        .ok()
        .is_some_and(|pgid| pgid.as_raw() == leader)
}

#[cfg(target_os = "linux")]
fn listen_pids(port: u16) -> Vec<u32> {
    let inodes = listen_inodes(port);
    if inodes.is_empty() {
        return Vec::new();
    }
    let mut pids = Vec::new();
    let Ok(proc) = std::fs::read_dir("/proc") else {
        return pids;
    };
    for entry in proc.flatten() {
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        if pid_holds_socket(pid, &inodes) {
            pids.push(pid);
        }
    }
    pids
}

#[cfg(target_os = "linux")]
fn listen_inodes(port: u16) -> Vec<u64> {
    let mut inodes = parse_proc_net("/proc/net/tcp", port);
    inodes.extend(parse_proc_net("/proc/net/tcp6", port));
    inodes
}

#[cfg(target_os = "linux")]
fn parse_proc_net(path: &str, port: u16) -> Vec<u64> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut inodes = Vec::new();
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 10 || cols[3] != "0A" {
            continue;
        }
        let Some((_, port_hex)) = cols[1].split_once(':') else {
            continue;
        };
        let Ok(local) = u16::from_str_radix(port_hex, 16) else {
            continue;
        };
        if local == port
            && let Ok(inode) = cols[9].parse()
        {
            inodes.push(inode);
        }
    }
    inodes
}

#[cfg(target_os = "linux")]
fn pid_holds_socket(pid: u32, inodes: &[u64]) -> bool {
    let Ok(fds) = std::fs::read_dir(format!("/proc/{pid}/fd")) else {
        return false;
    };
    for fd in fds.flatten() {
        let Ok(link) = std::fs::read_link(fd.path()) else {
            continue;
        };
        let Some(inode) = socket_inode(&link) else {
            continue;
        };
        if inodes.contains(&inode) {
            return true;
        }
    }
    false
}

#[cfg(target_os = "linux")]
fn socket_inode(path: &std::path::Path) -> Option<u64> {
    let name = path.to_str()?;
    name.strip_prefix("socket:[")?
        .strip_suffix(']')?
        .parse()
        .ok()
}

#[cfg(target_os = "macos")]
fn listen_pids(port: u16) -> Vec<u32> {
    let output = std::process::Command::new("/usr/sbin/lsof")
        .args(["-nP", "-sTCP:LISTEN", &format!("-iTCP:{port}"), "-t"])
        .output();
    let Ok(output) = output else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn listen_pids(_port: u16) -> Vec<u32> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn port_listen_names_this_process() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert_eq!(port_listen(port, std::process::id()), PortListen::Ours);
    }

    #[test]
    fn port_listen_flags_a_stranger() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert_eq!(port_listen(port, 1), PortListen::Stranger);
    }
}
