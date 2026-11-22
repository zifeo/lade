//! `lade attach`, at the remote end of the transport: serves the remote's
//! commands on a Unix socket and carries their requests to the laptop.

use anyhow::{Context, Result, bail};
use lade_sdk::upstream::{self, Frame, read_frame, write_frame};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};

use super::streams::{Out, Streams};
use super::{Marker, MarkerState};
use crate::message_box::Report;

/// The laptop pings every 15 s: three missed pings and the link is gone.
const HEARTBEAT_LOST: Duration = Duration::from_secs(45);
const HELLO_TIMEOUT: Duration = Duration::from_secs(15);
const LOCK_WAIT: Duration = Duration::from_secs(5);
/// A resumed link waits out the old attach's heartbeat instead of ending it:
/// an attach still holding the lock past that is another live link.
const RESUME_WAIT: Duration = Duration::from_secs(HEARTBEAT_LOST.as_secs() + 5);
/// Past the laptop hub's own wait for a tunnel to start, short of the
/// remote supervisor's: each side's error is shown before the next gives up.
const TUNNEL_TIMEOUT: Duration = Duration::from_secs(90);
const _: () = assert!(
    TUNNEL_TIMEOUT.as_millis() > crate::network::HUB_START_TIMEOUT.as_millis()
        && TUNNEL_TIMEOUT.as_millis() < crate::network::LINKED_STARTUP_TIMEOUT.as_millis()
);

type Pending = Arc<Mutex<HashMap<u64, oneshot::Sender<Frame>>>>;

fn hello() -> Frame {
    Frame::Hello {
        protocol: upstream::PROTOCOL,
        lade: env!("CARGO_PKG_VERSION").to_string(),
        host: super::hostname(),
    }
}

/// Refused before the handshake: the laptop reads it as a failure only a
/// human clears.
async fn refuse(message: String) -> Result<i32> {
    let mut stdout = tokio::io::stdout();
    stdout.write_all(upstream::MAGIC).await?;
    write_frame(
        &mut stdout,
        &Frame::Failed {
            id: 0,
            code: upstream::REFUSED,
            message,
        },
    )
    .await?;
    Ok(crate::exit_codes::FAILURE)
}

fn try_lock(
    file: std::fs::File,
) -> std::result::Result<nix::fcntl::Flock<std::fs::File>, std::fs::File> {
    nix::fcntl::Flock::lock(file, nix::fcntl::FlockArg::LockExclusiveNonblock)
        .map_err(|(file, _)| file)
}

pub(crate) async fn run(laptop: String, transport: String, resume: bool) -> Result<i32> {
    super::ensure_dir()?;
    if resume {
        // § q4qz9zrdn.a@3tqds
        if super::detached_path().exists() {
            return refuse(format!(
                "{} was detached with lade detach there. Run lade connect again from the laptop.",
                super::hostname()
            ))
            .await;
        }
    } else {
        match std::fs::remove_file(super::detached_path()) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => {}
        }
    }
    // § jzxqp2f1m@yvxgv
    if let MarkerState::Attached(marker) = super::read_marker()
        && marker.laptop != laptop
    {
        return refuse(format!(
            "{} is attached to {}. Run lade detach on {} first.",
            super::hostname(),
            marker.laptop,
            super::hostname()
        ))
        .await;
    }
    let lock_path = super::lock_path();
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)
        .with_context(|| format!("cannot open {}", lock_path.display()))?;
    let started = std::time::Instant::now();
    let mut replaced = false;
    let _lock = loop {
        match try_lock(file) {
            Ok(lock) => break lock,
            Err(back) => file = back,
        }
        // The same laptop connects again: its old link gives way. A resumed
        // link never displaces one, or two transports to this machine would
        // end each other in turn.
        // § dzex6kwmq.b@brhtf
        if !resume
            && !replaced
            && let MarkerState::Attached(marker) = super::read_marker()
        {
            replaced = true;
            end_attach(&marker);
        }
        if started.elapsed() > if resume { RESUME_WAIT } else { LOCK_WAIT } {
            return refuse(format!(
                "another lade attach on {} holds {}",
                super::hostname(),
                lock_path.display()
            ))
            .await;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let socket = super::socket_path();
    let _ = std::fs::remove_file(&socket);
    let listener =
        UnixListener::bind(&socket).with_context(|| format!("cannot bind {}", socket.display()))?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    }
    let marker = Marker {
        laptop,
        since: chrono::Utc::now().to_rfc3339(),
        protocol: upstream::PROTOCOL,
        transport,
        pid: std::process::id(),
    };
    super::write_marker(&marker)?;

    let mut stdout = tokio::io::stdout();
    let mut stdin = tokio::io::stdin();
    stdout.write_all(upstream::MAGIC).await?;
    write_frame(&mut stdout, &hello()).await?;
    match tokio::time::timeout(HELLO_TIMEOUT, read_frame(&mut stdin)).await {
        Ok(Ok(Some(Frame::Hello { protocol, .. }))) if protocol == upstream::PROTOCOL => {}
        Ok(Ok(Some(Frame::Hello { protocol, lade, .. }))) => bail!(
            "the laptop runs Lade {lade} (link protocol {protocol}) and this machine Lade {} (link protocol {})",
            env!("CARGO_PKG_VERSION"),
            upstream::PROTOCOL
        ),
        _ => bail!("the laptop did not answer the link handshake"),
    }

    let (out, _writer) = Out::spawn(stdout);
    let pending: Pending = Arc::default();
    let streams = Streams::default();
    let next_id = Arc::new(AtomicU64::new(1));
    {
        let out = out.clone();
        let pending = Arc::clone(&pending);
        let streams = streams.clone();
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                // § g1qjsg8hr@hpnre
                if crate::hub::peer_uid(&stream) != Some(crate::hub::current_uid()) {
                    continue;
                }
                let id = next_id.fetch_add(1, Ordering::Relaxed);
                tokio::spawn(serve_client(
                    stream,
                    id,
                    out.clone(),
                    Arc::clone(&pending),
                    streams.clone(),
                ));
            }
        });
    }

    let (in_tx, mut in_rx) = mpsc::unbounded_channel::<Option<Frame>>();
    tokio::spawn(async move {
        loop {
            let frame = read_frame(&mut stdin).await.ok().flatten();
            let end = frame.is_none();
            if in_tx.send(frame).is_err() || end {
                break;
            }
        }
    });
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let detached = loop {
        tokio::select! {
            frame = tokio::time::timeout(HEARTBEAT_LOST, in_rx.recv()) => match frame {
                Ok(Some(Some(Frame::Ping))) => {
                    out.send(Frame::Pong);
                }
                Ok(Some(Some(Frame::Detach))) => break true,
                Ok(Some(Some(frame))) => {
                    if let Some(id) = reply_id(&frame) {
                        let waiting = pending.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
                        // An answer past its client's wait: the laptop would
                        // keep that tunnel's lease, or that stream, for nobody.
                        let orphan = match waiting {
                            Some(waiting) => waiting.send(frame).err(),
                            None => Some(frame),
                        };
                        match orphan {
                            Some(Frame::TunnelReady { id }) => {
                                out.send(Frame::TunnelClose { id });
                            }
                            Some(Frame::StreamReady { id }) => {
                                out.send(Frame::Reset { id });
                            }
                            _ => {}
                        }
                    } else {
                        streams.route(&frame);
                    }
                }
                Ok(Some(None) | None) | Err(_) => break false,
            },
            _ = term.recv() => {
                // `lade detach` removed the marker first: tell the laptop.
                // A new attach left its own marker: leave it.
                match super::read_marker() {
                    MarkerState::None => {
                        out.send(Frame::Detach);
                        tokio::time::sleep(Duration::from_millis(200)).await;
                    }
                    MarkerState::Attached(_) | MarkerState::Unreadable(_) => {}
                }
                break false;
            }
        }
    };
    let _ = std::fs::remove_file(&socket);
    if detached
        && matches!(super::read_marker(), MarkerState::Attached(ref m) if m.pid == std::process::id())
    {
        let _ = std::fs::remove_file(super::marker_path());
    }
    Ok(0)
}

/// The id of a frame that answers a client's request.
fn reply_id(frame: &Frame) -> Option<u64> {
    match frame {
        Frame::Resolved { id, .. }
        | Frame::Failed { id, .. }
        | Frame::TunnelReady { id }
        | Frame::StreamReady { id } => Some(*id),
        _ => None,
    }
}

async fn serve_client(
    mut stream: UnixStream,
    id: u64,
    out: Out,
    pending: Pending,
    streams: Streams,
) {
    match read_frame(&mut stream).await {
        Ok(Some(Frame::Hello { .. })) => {
            if write_frame(&mut stream, &hello()).await.is_err() {
                return;
            }
        }
        _ => return,
    }
    let Ok(Some(first)) = read_frame(&mut stream).await else {
        return;
    };
    match first {
        Frame::Resolve {
            walk,
            refs,
            recipient,
            report,
            ..
        } => {
            let ask = Frame::Resolve {
                id,
                walk,
                refs,
                recipient,
                report,
            };
            let reply = relay(ask, id, &out, &pending, upstream::UPSTREAM_TIMEOUT).await;
            let _ = write_frame(&mut stream, &reply).await;
        }
        Frame::TunnelOpen {
            walk,
            uri,
            check,
            report,
            ..
        } => {
            let ask = Frame::TunnelOpen {
                id,
                walk,
                uri,
                check,
                report,
            };
            let reply = relay(ask, id, &out, &pending, TUNNEL_TIMEOUT).await;
            let open = matches!(reply, Frame::TunnelReady { .. }) && !check;
            if write_frame(&mut stream, &reply).await.is_ok() && open {
                // Held until the client goes: the laptop keeps its lease.
                let mut byte = [0u8; 1];
                while matches!(stream.read(&mut byte).await, Ok(n) if n > 0) {}
            }
            if open {
                out.send(Frame::TunnelClose { id });
            }
        }
        Frame::StreamOpen { tunnel, .. } => {
            // Registered first: the laptop may send bytes right after ready.
            let opened = streams.open(id);
            let reply = relay(
                Frame::StreamOpen { id, tunnel },
                id,
                &out,
                &pending,
                TUNNEL_TIMEOUT,
            )
            .await;
            let ready = matches!(reply, Frame::StreamReady { .. });
            if write_frame(&mut stream, &reply).await.is_err() || !ready {
                streams.close(id);
                if ready {
                    out.send(Frame::Reset { id });
                }
                return;
            }
            streams.pump(stream, id, opened, out).await;
        }
        _ => {}
    }
}

/// Sends `ask` to the laptop and waits for its answer, a failure frame on
/// any error.
async fn relay(ask: Frame, id: u64, out: &Out, pending: &Pending, wait: Duration) -> Frame {
    let (tx, rx) = oneshot::channel();
    pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, tx);
    let message = if !out.send(ask) {
        "the link is closing".to_string()
    } else {
        match tokio::time::timeout(wait, rx).await {
            Ok(Ok(frame)) => return frame,
            Ok(Err(_)) => "the link closed before the laptop answered".to_string(),
            Err(_) => format!("the laptop did not answer within {}s", wait.as_secs()),
        }
    };
    pending
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&id);
    Frame::Failed {
        id: 0,
        code: upstream::ABSENT,
        message,
    }
}

/// SIGTERM to the attach the marker names, only while some attach holds the
/// lock: a stale marker's pid may be another process by now, and pid 0 or a
/// negative one would signal a whole process group.
fn end_attach(marker: &Marker) {
    let Ok(pid) = i32::try_from(marker.pid) else {
        return;
    };
    if pid <= 1 {
        return;
    }
    let held = std::fs::OpenOptions::new()
        .write(true)
        .open(super::lock_path())
        .is_ok_and(|file| try_lock(file).is_err());
    if held {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGTERM,
        );
    }
}

/// `lade detach` on the remote.
pub(crate) fn detach_here() -> Result<()> {
    // § q4qz9zrdn@0hbgn
    super::ensure_dir()?;
    std::fs::write(super::detached_path(), b"")?;
    match super::read_marker() {
        MarkerState::None => {
            Report::new()
                .line("This machine is not attached to a laptop.")
                .print();
        }
        MarkerState::Unreadable(why) => {
            std::fs::remove_file(super::marker_path())?;
            Report::new()
                .line(format!("Removed an unreadable attach marker ({why})."))
                .print();
        }
        MarkerState::Attached(marker) => {
            std::fs::remove_file(super::marker_path())?;
            end_attach(&marker);
            Report::new()
                .line(format!(
                    "Detached from {}. Secrets resolve on this machine again.",
                    marker.laptop
                ))
                .print();
        }
    }
    Ok(())
}
