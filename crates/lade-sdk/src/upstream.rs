//! A remote machine attached to a laptop resolves its secret refs there.
//! `lade attach` serves a Unix socket on the remote and carries frames over
//! the transport to `lade __remote` on the laptop. This module holds the frames
//! both ends share and the provider that replaces every secret provider on
//! an attached remote.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::RwLock;
use std::time::Duration;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::Hydration;
use crate::providers::{HUMAN_PROVIDER_TIMEOUT, Provider, Warnings};

/// Apart from the hub's wire: laptop and remote upgrade on their own.
pub const PROTOCOL: u32 = 1;

/// Written by `lade attach` before any frame, so shell rc output that
/// reaches the pipe first is skipped instead of read as a frame.
pub const MAGIC: &[u8] = b"\x00LADE-LINK\n";

/// Bytes before [`MAGIC`] past which the handshake fails.
pub const MAGIC_SCAN_CAP: usize = 64 * 1024;

const MAX_FRAME: u32 = 16 * 1024 * 1024;

/// Above the laptop's own provider bound, so a Touch ID prompt there does
/// not fail the first request.
// § a36hqp4ep.c@w7g37
pub const UPSTREAM_TIMEOUT: Duration = Duration::from_secs(HUMAN_PROVIDER_TIMEOUT.as_secs() + 5);
// § end a36hqp4ep.c

const ATTACH_GRACE: Duration = Duration::from_secs(2);

/// Exit codes, also in `src/exit_codes.rs`.
pub const ABSENT: u8 = 4;
pub const REFUSED: u8 = 5;
pub const WALK_UNKNOWN: u8 = 6;

/// Every secret scheme whose provider needs a credential. `file`, `sops`
/// and the shells read or run on the remote itself: their files are there.
// § a36hqp4ep.a@wjggx
pub const SCHEMES: &[&str] = &[
    "doppler",
    "infisical",
    "op",
    "vault",
    "passbolt",
    "awssm",
    "azurekv",
    "bw",
    "gcpsm",
    "age",
];
// § end a36hqp4ep.a

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Frame {
    Hello {
        protocol: u32,
        lade: String,
        host: String,
    },
    Resolve {
        id: u64,
        walk: String,
        refs: Vec<String>,
        recipient: String,
        report: serde_json::Value,
    },
    Resolved {
        id: u64,
        sealed: String,
    },
    Failed {
        id: u64,
        code: u8,
        message: String,
    },
    /// Opens a laptop tunnel for a remote one, held while the asking client
    /// stays connected. `check`: the laptop only answers whether it would.
    TunnelOpen {
        id: u64,
        walk: String,
        uri: String,
        check: bool,
        report: serde_json::Value,
    },
    TunnelReady {
        id: u64,
    },
    TunnelClose {
        id: u64,
    },
    /// One connection to an open tunnel, `id` its own.
    StreamOpen {
        id: u64,
        tunnel: u64,
    },
    StreamReady {
        id: u64,
    },
    /// Connection bytes, base64.
    Data {
        id: u64,
        data: String,
    },
    /// The sender writes nothing more on this stream.
    End {
        id: u64,
    },
    /// The stream is gone both ways.
    Reset {
        id: u64,
    },
    /// The receiver wrote `n` more `Data` frames of stream `id` to its
    /// socket: the sender may send that many more.
    Ack {
        id: u64,
        n: u32,
    },
    Ping,
    Pong,
    Detach,
}

pub async fn write_frame<W: AsyncWrite + Unpin>(out: &mut W, frame: &Frame) -> Result<()> {
    let body = serde_json::to_vec(frame)?;
    let n = u32::try_from(body.len())
        .ok()
        .filter(|n| *n <= MAX_FRAME)
        .ok_or_else(|| anyhow!("link frame of {} bytes is too large", body.len()))?;
    out.write_all(&n.to_be_bytes()).await?;
    out.write_all(&body).await?;
    out.flush().await?;
    Ok(())
}

/// `None` at a clean end of stream.
pub async fn read_frame<R: AsyncRead + Unpin>(input: &mut R) -> Result<Option<Frame>> {
    let mut header = [0u8; 4];
    match input.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    }
    let n = u32::from_be_bytes(header);
    if n > MAX_FRAME {
        anyhow::bail!("link frame of {n} bytes is too large");
    }
    let mut body = vec![0u8; n as usize];
    input.read_exact(&mut body).await?;
    Ok(Some(serde_json::from_slice(&body)?))
}

/// Reads up to and including [`MAGIC`].
pub async fn skip_to_magic<R: AsyncRead + Unpin>(input: &mut R) -> Result<()> {
    let mut seen = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if input.read(&mut byte).await? == 0 {
            anyhow::bail!("the remote closed the link before Lade answered");
        }
        seen.push(byte[0]);
        if seen.ends_with(MAGIC) {
            return Ok(());
        }
        if seen.len() > MAGIC_SCAN_CAP {
            anyhow::bail!(
                "the remote printed {MAGIC_SCAN_CAP} bytes before Lade answered: check its shell rc files for output"
            );
        }
    }
}

/// Encrypts `plaintext` to the key the requesting client generated.
pub fn seal(recipient: &str, plaintext: &[u8]) -> Result<String> {
    let recipient = age::x25519::Recipient::from_str(recipient)
        .map_err(|e| anyhow!("invalid request key: {e}"))?;
    Ok(age::encrypt_and_armor(&recipient, plaintext)?)
}

/// The exit code a failed link request ends the command with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamError {
    pub code: u8,
    pub message: String,
}

impl std::fmt::Display for UpstreamError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for UpstreamError {}

/// What `lade attach` left on the remote. `broken`: the marker could not be
/// read, so every request fails as if the link were down.
#[derive(Debug, Clone)]
pub struct Attached {
    pub socket: PathBuf,
    pub laptop: String,
    pub connect: String,
    pub broken: Option<String>,
    /// The walk hash of the command's `lade.yaml` files, hex.
    pub walk: fn() -> Result<String>,
    /// What the remote reports about the command, for the laptop's diary.
    pub report: fn() -> serde_json::Value,
}

static ATTACHED: RwLock<Option<Attached>> = RwLock::new(None);

/// From here on, every secret scheme in [`SCHEMES`] resolves through the
/// link.
pub fn attach(attached: Attached) {
    *ATTACHED.write().unwrap_or_else(|e| e.into_inner()) = Some(attached);
}

pub fn attached() -> Option<Attached> {
    ATTACHED.read().unwrap_or_else(|e| e.into_inner()).clone()
}

pub(crate) fn takes(scheme: &str) -> bool {
    SCHEMES.contains(&scheme)
}

/// True when `scheme` resolves through the laptop here: this machine then
/// needs no CLI of its own for it.
pub fn routes(scheme: &str) -> bool {
    takes(scheme) && attached().is_some()
}

/// Exit 4: the link `attached` names is down. `named`: what was asked.
pub fn absent(attached: &Attached, named: &[String], why: &str) -> anyhow::Error {
    // § a36hqp4ep.b@mjbgp
    let named = match named {
        [] => String::new(),
        [one] => format!(" ({one})"),
        [first, rest @ ..] => format!(" ({first}, +{})", rest.len()),
    };
    UpstreamError {
        code: ABSENT,
        message: format!(
            "This machine is attached to {} but the link is down{named}: {why}. Ask the user to run on the laptop: {}. Or stop using the laptop here: lade detach",
            attached.laptop, attached.connect
        ),
    }
    .into()
}

/// The error of a `Failed` frame, with the connect line that resolves the
/// walk again.
pub fn failed(attached: &Attached, code: u8, message: String) -> anyhow::Error {
    let message = match code {
        ABSENT | REFUSED | WALK_UNKNOWN => format!(
            "{}. Ask the user to run on the laptop: {}",
            message.trim_end_matches('.'),
            attached.connect
        ),
        _ => message,
    };
    UpstreamError { code, message }.into()
}

/// A connection to `lade attach`, past the hello.
pub async fn session(attached: &Attached, named: &[String]) -> Result<tokio::net::UnixStream> {
    if let Some(why) = &attached.broken {
        return Err(absent(attached, named, why));
    }
    let mut stream = match tokio::net::UnixStream::connect(&attached.socket).await {
        Ok(stream) => stream,
        Err(_) => return Err(absent(attached, named, "lade attach is not running")),
    };
    write_frame(
        &mut stream,
        &Frame::Hello {
            protocol: PROTOCOL,
            lade: crate::VERSION.to_string(),
            host: String::new(),
        },
    )
    .await?;
    match read_frame(&mut stream).await? {
        Some(Frame::Hello { protocol, .. }) if protocol == PROTOCOL => Ok(stream),
        Some(Frame::Hello { protocol, lade, .. }) => anyhow::bail!(
            "lade attach runs Lade {lade} (link protocol {protocol}) and this command Lade {} (link protocol {PROTOCOL}). Run lade connect again from the laptop.",
            crate::VERSION
        ),
        Some(Frame::Failed { code, message, .. }) => Err(UpstreamError { code, message }.into()),
        _ => Err(absent(attached, named, "lade attach did not answer")),
    }
}

pub(crate) struct Upstream {
    attached: Attached,
    refs: FxHashSet<String>,
}

impl Upstream {
    pub(crate) fn new(attached: Attached) -> Self {
        Self {
            attached,
            refs: FxHashSet::default(),
        }
    }

    fn absent(&self, why: &str) -> anyhow::Error {
        let mut refs = self.refs.iter().cloned().collect::<Vec<_>>();
        refs.sort();
        absent(&self.attached, &refs, why)
    }

    async fn request(&self) -> Result<Hydration> {
        let walk = (self.attached.walk)()?;
        let mut refs = self.refs.iter().cloned().collect::<Vec<_>>();
        refs.sort();
        let mut stream = session(&self.attached, &refs).await?;
        let key = age::x25519::Identity::generate();
        write_frame(
            &mut stream,
            &Frame::Resolve {
                id: 0,
                walk,
                refs,
                recipient: key.to_public().to_string(),
                report: (self.attached.report)(),
            },
        )
        .await?;
        // `lade attach` gives up first, at UPSTREAM_TIMEOUT: this bound only
        // catches an attach that stopped answering at all.
        let answer = tokio::time::timeout(UPSTREAM_TIMEOUT + ATTACH_GRACE, read_frame(&mut stream))
            .await
            .map_err(|_| self.absent("lade attach stopped answering"))??;
        match answer {
            Some(Frame::Resolved { sealed, .. }) => {
                let plain = age::decrypt(&key, sealed.as_bytes())
                    .map_err(|e| anyhow!("cannot open the laptop's answer: {e}"))?;
                let values: HashMap<String, String> = serde_json::from_slice(&plain)?;
                Ok(values.into_iter().collect())
            }
            Some(Frame::Failed { code, message, .. }) => Err(failed(&self.attached, code, message)),
            _ => Err(self.absent("the link closed before the laptop answered")),
        }
    }
}

#[async_trait]
impl Provider for Upstream {
    fn add(&mut self, value: String) -> Result<()> {
        self.refs.insert(value);
        Ok(())
    }

    fn name(&self) -> &'static str {
        "the laptop"
    }

    fn timeout(&self) -> Duration {
        UPSTREAM_TIMEOUT + 2 * ATTACH_GRACE
    }

    fn install_url(&self) -> &'static str {
        "https://github.com/zifeo/lade"
    }

    fn batch_unit(&self) -> &'static str {
        "link"
    }

    fn has_work(&self) -> bool {
        !self.refs.is_empty()
    }

    async fn resolve(
        &self,
        _cwd: &Path,
        _extra_env: &HashMap<String, String>,
        _warnings: &Warnings,
    ) -> Result<Hydration> {
        self.request().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frames_round_trip_after_noise() {
        let mut bytes = b"motd from .bashrc\n".to_vec();
        bytes.extend_from_slice(MAGIC);
        let mut framed = Vec::new();
        write_frame(&mut framed, &Frame::Ping).await.unwrap();
        bytes.extend_from_slice(&framed);
        let mut input = bytes.as_slice();
        skip_to_magic(&mut input).await.unwrap();
        assert_eq!(read_frame(&mut input).await.unwrap(), Some(Frame::Ping));
        assert_eq!(read_frame(&mut input).await.unwrap(), None);
    }

    #[test]
    fn sealed_values_open_only_with_the_request_key() {
        let key = age::x25519::Identity::generate();
        let sealed = seal(&key.to_public().to_string(), b"hunter2").unwrap();
        assert_eq!(age::decrypt(&key, sealed.as_bytes()).unwrap(), b"hunter2");
        let other = age::x25519::Identity::generate();
        assert!(age::decrypt(&other, sealed.as_bytes()).is_err());
    }
}
