use anyhow::Result;
use nix::errno::Errno;
use nix::sys::socket::{
    AddressFamily, SockFlag, SockType, SockaddrStorage, setsockopt, socket, sockopt,
};
use std::io::ErrorKind;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, ToSocketAddrs};
use std::os::fd::AsRawFd;

/// The user-facing port, bound by Lade. Holding it is the claim: a second
/// tunnel gets `AddrInUse` at once, and the kernel frees the port when the
/// holder dies. No lock file.
#[derive(Debug)]
pub(crate) struct Outer {
    pub(crate) listeners: Vec<TcpListener>,
    pub(crate) host: String,
    pub(crate) port: u16,
}

#[derive(Debug)]
pub(crate) enum ClaimError {
    InUse { port: u16 },
    Other(anyhow::Error),
}

impl Outer {
    /// `port: None` takes a free port.
    pub(crate) fn claim(host: &str, port: Option<u16>) -> Result<Self, ClaimError> {
        if let Some(port) = port {
            return Self::assemble(host, port, Vec::new());
        }
        Self::claim_ephemeral(host)
    }

    fn claim_ephemeral(host: &str) -> Result<Self, ClaimError> {
        // Keep the kernel's pick. Dropping it and rebinding is a hole.
        let ip = resolve(host, 0)?[0].ip();
        for _ in 0..EPHEMERAL_TRIES {
            let first = TcpListener::bind((ip, 0))
                .map_err(|e| ClaimError::Other(anyhow::anyhow!("no free port on {ip}: {e}")))?;
            let port = first
                .local_addr()
                .map_err(|e| ClaimError::Other(anyhow::anyhow!("no free port on {ip}: {e}")))?
                .port();
            match Self::assemble(host, port, vec![first]) {
                Err(ClaimError::InUse { .. }) => continue,
                claimed => return claimed,
            }
        }
        Err(ClaimError::Other(anyhow::anyhow!(
            "no free local port on {host} after {EPHEMERAL_TRIES} tries"
        )))
    }

    fn assemble(
        host: &str,
        port: u16,
        mut listeners: Vec<TcpListener>,
    ) -> Result<Self, ClaimError> {
        let addrs = resolve(host, port)?;
        let held: Vec<SocketAddr> = listeners
            .iter()
            .filter_map(|listener| listener.local_addr().ok())
            .collect();
        for ip in PROBES {
            if held.iter().any(|addr| addr.ip() == ip) {
                continue;
            }
            if !addrs.iter().any(|addr| addr.ip() == ip) {
                probe(ip, port)?;
            }
        }
        let mut unbound = None;
        for addr in &addrs {
            if held.iter().any(|held| held == addr) {
                continue;
            }
            match bind(*addr, port) {
                Ok(listener) => listeners.push(listener),
                Err(ClaimError::InUse { port }) => return Err(ClaimError::InUse { port }),
                // `localhost` may resolve to ::1 first on a host without IPv6.
                Err(other) => unbound = unbound.or(Some(other)),
            }
        }
        if listeners.is_empty() {
            return Err(unbound.expect("resolve returns at least one address"));
        }
        Ok(Self {
            listeners,
            host: host.to_string(),
            port,
        })
    }
}

const EPHEMERAL_TRIES: usize = 16;

/// At least one address.
fn resolve(host: &str, port: u16) -> Result<Vec<SocketAddr>, ClaimError> {
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|e| ClaimError::Other(e.into()))?
        .collect::<Vec<_>>();
    if addrs.is_empty() {
        return Err(ClaimError::Other(anyhow::anyhow!(
            "local host '{host}' did not resolve"
        )));
    }
    Ok(addrs)
}

/// With SO_REUSEADDR, macOS refuses only the identical address:
/// `127.0.0.1:P` binds while `0.0.0.0:P` or `[::]:P` listens, and a
/// `localhost` client may reach either. So a port is held when any of
/// these is taken.
const PROBES: [IpAddr; 4] = [
    IpAddr::V4(Ipv4Addr::UNSPECIFIED),
    IpAddr::V6(Ipv6Addr::UNSPECIFIED),
    IpAddr::V4(Ipv4Addr::LOCALHOST),
    IpAddr::V6(Ipv6Addr::LOCALHOST),
];

/// Binds without listening, with SO_REUSEPORT: two claims probing one port
/// at once pass each other, and only a real holder refuses the bind.
fn probe(ip: IpAddr, port: u16) -> Result<(), ClaimError> {
    let family = match ip {
        IpAddr::V4(_) => AddressFamily::Inet,
        IpAddr::V6(_) => AddressFamily::Inet6,
    };
    // A provider another hub thread spawns meanwhile must not inherit a
    // socket bound to the port: it would hold the port for its whole life.
    #[cfg(target_os = "linux")]
    let flags = SockFlag::SOCK_CLOEXEC;
    #[cfg(not(target_os = "linux"))]
    let flags = SockFlag::empty();
    // A host without IPv6 cannot open or bind `::`.
    let Ok(fd) = socket(family, SockType::Stream, flags, None) else {
        return Ok(());
    };
    #[cfg(not(target_os = "linux"))]
    let _ = nix::fcntl::fcntl(
        &fd,
        nix::fcntl::FcntlArg::F_SETFD(nix::fcntl::FdFlag::FD_CLOEXEC),
    );
    let _ = setsockopt(&fd, sockopt::ReuseAddr, &true);
    let _ = setsockopt(&fd, sockopt::ReusePort, &true);
    // Dual-stack [::] would see our own 127.0.0.1 holder as InUse.
    if family == AddressFamily::Inet6 {
        let _ = setsockopt(&fd, sockopt::Ipv6V6Only, &true);
    }
    let addr = SockaddrStorage::from(SocketAddr::new(ip, port));
    match nix::sys::socket::bind(fd.as_raw_fd(), &addr) {
        Err(Errno::EADDRINUSE) => Err(ClaimError::InUse { port }),
        _ => Ok(()),
    }
}

fn bind(addr: SocketAddr, port: u16) -> Result<TcpListener, ClaimError> {
    TcpListener::bind(addr).map_err(|e| match e.kind() {
        ErrorKind::AddrInUse => ClaimError::InUse { port },
        _ => ClaimError::Other(anyhow::anyhow!("could not listen on {addr}: {e}")),
    })
}

/// A port no address holds right now, for a test to claim by number.
#[cfg(test)]
pub(crate) fn free_fixed_port() -> u16 {
    Outer::claim("127.0.0.1", None).expect("free port").port
}
