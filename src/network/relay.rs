use anyhow::Result;
use nix::errno::Errno;
use nix::sys::socket::{
    AddressFamily, SockFlag, SockType, SockaddrStorage, setsockopt, socket, sockopt,
};
use std::io::{self, ErrorKind};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use super::process::{EndpointSlot, Stream};

const UPSTREAM_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// The user-facing port, bound by Lade. Holding it is the claim: a second
/// tunnel gets `AddrInUse` at once, and the kernel frees the port when the
/// holder dies. No lock file.
#[derive(Debug)]
pub(crate) struct Outer {
    listeners: Vec<TcpListener>,
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
            return Self::claim_port(host, port);
        }
        // The kernel's pick only avoids the identical address, so it may be
        // held on another one: check it like a fixed port.
        let ip = resolve(host, 0)?[0].ip();
        for _ in 0..EPHEMERAL_TRIES {
            let picked = TcpListener::bind((ip, 0))
                .and_then(|listener| listener.local_addr())
                .map_err(|e| ClaimError::Other(anyhow::anyhow!("no free port on {ip}: {e}")))?
                .port();
            match Self::claim_port(host, picked) {
                Err(ClaimError::InUse { .. }) => continue,
                claimed => return claimed,
            }
        }
        Err(ClaimError::Other(anyhow::anyhow!(
            "no free local port on {host} after {EPHEMERAL_TRIES} tries"
        )))
    }

    fn claim_port(host: &str, port: u16) -> Result<Self, ClaimError> {
        let addrs = resolve(host, port)?;
        for ip in PROBES {
            if !addrs.iter().any(|addr| addr.ip() == ip) {
                probe(ip, port)?;
            }
        }
        let mut listeners = Vec::new();
        let mut unbound = None;
        for addr in &addrs {
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

/// Accepts on [`Outer`] and pipes each connection to whatever the provider
/// listens on right now, so a provider restart keeps the port.
#[derive(Debug)]
pub(crate) struct Relay {
    /// Dropping it wakes every accept thread at once. A wake by connecting
    /// to the port would fail once its address is gone (VPN down).
    wake: Option<UnixStream>,
    accepts: Vec<JoinHandle<()>>,
}

impl Relay {
    pub(crate) fn start(outer: Outer, upstream: EndpointSlot) -> Result<Self> {
        let (wake, woken) = UnixStream::pair()?;
        let mut accepts = Vec::new();
        for listener in outer.listeners {
            listener.set_nonblocking(true)?;
            let woken = woken.try_clone()?;
            let upstream = Arc::clone(&upstream);
            accepts.push(std::thread::spawn(move || {
                accept_loop(listener, &woken, &upstream)
            }));
        }
        Ok(Self {
            wake: Some(wake),
            accepts,
        })
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        drop(self.wake.take());
        for handle in self.accepts.drain(..) {
            let _ = handle.join();
        }
    }
}

/// Blocks until a client connects or the relay stops. False once stopped.
fn wait_for_client(listener: &TcpListener, woken: &UnixStream) -> bool {
    let mut fds = [listener.as_raw_fd(), woken.as_raw_fd()].map(|fd| libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    });
    loop {
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } >= 0 {
            return fds[1].revents == 0;
        }
        let error = io::Error::last_os_error();
        if error.kind() != ErrorKind::Interrupted {
            log::warn!("tunnel accept stopped: {error}");
            return false;
        }
    }
}

fn accept_loop(listener: TcpListener, woken: &UnixStream, upstream: &EndpointSlot) {
    while wait_for_client(&listener, woken) {
        // BSD sockets inherit the listener's O_NONBLOCK.
        let client = match listener
            .accept()
            .and_then(|(client, _)| client.set_nonblocking(false).map(|()| client))
        {
            Ok(client) => client,
            Err(e) if e.kind() == ErrorKind::WouldBlock => continue,
            Err(e) => {
                log::warn!("tunnel accept failed: {e}");
                std::thread::sleep(Duration::from_millis(50));
                continue;
            }
        };
        let endpoint = upstream
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone();
        std::thread::spawn(move || {
            let Some(endpoint) = endpoint else {
                return;
            };
            match endpoint.connect(UPSTREAM_CONNECT_TIMEOUT) {
                Ok(upstream) => pipe(client, upstream),
                Err(e) => log::warn!("tunnel upstream {endpoint:?} refused: {e}"),
            }
        });
    }
}

fn pipe(client: TcpStream, upstream: Stream) {
    let (Ok(client_back), Ok(upstream_back)) = (client.try_clone(), upstream.try_clone()) else {
        return;
    };
    let back = std::thread::spawn(move || {
        let mut from = upstream_back;
        let mut to = Stream::Tcp(client_back);
        let _ = io::copy(&mut from, &mut to);
        to.shutdown_write();
    });
    let mut from = Stream::Tcp(client);
    let mut to = upstream;
    let _ = io::copy(&mut from, &mut to);
    to.shutdown_write();
    let _ = back.join();
}

/// A port no address holds right now, for a test to claim by number.
#[cfg(test)]
pub(crate) fn free_fixed_port() -> u16 {
    Outer::claim("127.0.0.1", None).expect("free port").port
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::process::Endpoint;
    use std::io::{Read, Write};
    use std::sync::Mutex;

    #[test]
    fn second_claim_on_a_port_is_refused() {
        let first = Outer::claim("127.0.0.1", None).unwrap();
        let err = Outer::claim("127.0.0.1", Some(first.port)).unwrap_err();
        assert!(matches!(err, ClaimError::InUse { port } if port == first.port));
    }

    #[test]
    fn loopback_claim_is_refused_while_a_wildcard_listens() {
        for wildcard in ["0.0.0.0", "::"] {
            let Ok(other) = TcpListener::bind((wildcard, 0)) else {
                continue;
            };
            let port = other.local_addr().unwrap().port();
            let err = Outer::claim("127.0.0.1", Some(port)).unwrap_err();
            assert!(
                matches!(err, ClaimError::InUse { port: held } if held == port),
                "{wildcard}"
            );
        }
    }

    #[test]
    fn wildcard_claim_is_refused_while_loopback_listens() {
        let other = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = other.local_addr().unwrap().port();
        let err = Outer::claim("0.0.0.0", Some(port)).unwrap_err();
        assert!(matches!(err, ClaimError::InUse { port: held } if held == port));
    }

    #[test]
    fn relay_stops_without_a_connection() {
        let outer = Outer::claim("127.0.0.1", None).unwrap();
        let relay = Relay::start(outer, Arc::new(Mutex::new(None))).unwrap();
        let stopped = std::time::Instant::now();
        drop(relay);
        assert!(stopped.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn dropped_claim_frees_the_port() {
        // A test alongside may take the port in between. A claim that kept
        // its port would fail every try.
        let freed = (0..8).any(|_| {
            let port = Outer::claim("127.0.0.1", None).unwrap().port;
            Outer::claim("127.0.0.1", Some(port)).is_ok()
        });
        assert!(freed);
    }

    #[test]
    fn dropped_relay_frees_the_port() {
        let freed = (0..8).any(|_| {
            let outer = Outer::claim("127.0.0.1", None).unwrap();
            let port = outer.port;
            drop(Relay::start(outer, Arc::new(Mutex::new(None))).unwrap());
            Outer::claim("127.0.0.1", Some(port)).is_ok()
        });
        assert!(freed);
    }

    #[test]
    fn relay_pipes_to_the_current_upstream() {
        let upstream = TcpListener::bind("127.0.0.1:0").unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        let echo = std::thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            let mut buf = [0u8; 4];
            stream.read_exact(&mut buf).unwrap();
            stream.write_all(&buf).unwrap();
        });
        let outer = Outer::claim("127.0.0.1", None).unwrap();
        let port = outer.port;
        let slot = Arc::new(Mutex::new(Some(Endpoint::Tcp {
            host: "127.0.0.1".to_string(),
            port: upstream_port,
        })));
        let relay = Relay::start(outer, slot).unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client.write_all(b"ping").unwrap();
        let mut back = [0u8; 4];
        client.read_exact(&mut back).unwrap();
        assert_eq!(&back, b"ping");
        echo.join().unwrap();
        drop(relay);
    }

    #[test]
    fn relay_pipes_to_a_unix_socket_upstream() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s.sock");
        let upstream = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let echo = std::thread::spawn(move || {
            let (mut stream, _) = upstream.accept().unwrap();
            let mut buf = [0u8; 4];
            stream.read_exact(&mut buf).unwrap();
            stream.write_all(&buf).unwrap();
        });
        let outer = Outer::claim("127.0.0.1", None).unwrap();
        let port = outer.port;
        let slot = Arc::new(Mutex::new(Some(Endpoint::Unix(path))));
        let _relay = Relay::start(outer, slot).unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client.write_all(b"pong").unwrap();
        let mut back = [0u8; 4];
        client.read_exact(&mut back).unwrap();
        assert_eq!(&back, b"pong");
        echo.join().unwrap();
    }

    #[test]
    fn relay_drops_connections_while_the_provider_restarts() {
        let outer = Outer::claim("127.0.0.1", None).unwrap();
        let port = outer.port;
        let _relay = Relay::start(outer, Arc::new(Mutex::new(None))).unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let mut buf = [0u8; 1];
        assert_eq!(client.read(&mut buf).unwrap(), 0);
    }
}
