mod child;
mod forward;
mod logs;
mod policy;

#[cfg(test)]
mod tests;

pub(crate) use forward::{EndpointSlot, RunningForward, Stop};

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// Delay between readiness checks while waiting for a forwarded port to
/// accept connections. Kept short for snappier detection; the dominant cost
/// is the provider CLI starting up, not this poll loop.
const READINESS_POLL_INTERVAL: Duration = Duration::from_millis(30);

pub(crate) fn configure_child_process(command: &mut Command) {
    #[cfg(target_family = "unix")]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            command.pre_exec(|| {
                nix::unistd::setsid().map_err(|e| std::io::Error::other(e.to_string()))?;
                Ok(())
            });
        }
    }
}

/// Where a provider listens once it is ready.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Endpoint {
    Tcp { host: String, port: u16 },
    Unix(PathBuf),
}

/// What the supervisor waits for before the provider counts as ready.
#[derive(Debug, Clone)]
pub(crate) enum Inner {
    /// The provider picks the port and prints it on stdout.
    Printed,
    Fixed(Endpoint),
}

impl Endpoint {
    pub(crate) fn connect(&self, timeout: Duration) -> io::Result<Stream> {
        match self {
            Self::Tcp { host, port } => {
                let mut last = io::Error::other(format!("{host}:{port} did not resolve"));
                for addr in (host.as_str(), *port).to_socket_addrs()? {
                    match TcpStream::connect_timeout(&addr, timeout) {
                        Ok(stream) => return Ok(Stream::Tcp(stream)),
                        Err(e) => last = e,
                    }
                }
                Err(last)
            }
            Self::Unix(path) => UnixStream::connect(path).map(Stream::Unix),
        }
    }
}

pub(crate) enum Stream {
    Tcp(TcpStream),
    Unix(UnixStream),
}

impl Stream {
    pub(crate) fn try_clone(&self) -> io::Result<Self> {
        match self {
            Self::Tcp(stream) => stream.try_clone().map(Self::Tcp),
            Self::Unix(stream) => stream.try_clone().map(Self::Unix),
        }
    }

    pub(crate) fn shutdown_write(&self) {
        let _ = match self {
            Self::Tcp(stream) => stream.shutdown(Shutdown::Write),
            Self::Unix(stream) => stream.shutdown(Shutdown::Write),
        };
    }
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.read(buf),
            Self::Unix(stream) => stream.read(buf),
        }
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::Tcp(stream) => stream.write(buf),
            Self::Unix(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::Tcp(stream) => stream.flush(),
            Self::Unix(stream) => stream.flush(),
        }
    }
}
