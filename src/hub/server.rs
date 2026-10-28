use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};

use super::peer;
use super::release::spawn_version_fetch;
use super::tables::{Tables, now, since};
use super::wire::{self, ERR_DENIED, Rep, Req};

const IDLE_EXIT: Duration = Duration::from_secs(3600);
// An unreachable hub has nothing left to wait for, so it does not wait out
// `IDLE_EXIT`.
const REACH_CHECK: Duration = Duration::from_secs(60);
const TUNNEL_TICK: Duration = Duration::from_secs(1);
// Clients give up after 20 ms. One that stalls must not hold every other
// client, and the idle and reach checks, behind it.
const CONN_TIMEOUT: Duration = Duration::from_millis(500);

enum AfterConn {
    Continue,
    Quit,
}

/// Returns when idle, orphaned, quit, or once `stop` resolves. Tunnels
/// stop their providers as the tables drop.
pub async fn serve(stop: impl Future<Output = ()>) -> anyhow::Result<()> {
    serve_until_idle(IDLE_EXIT, stop).await
}

pub(super) async fn serve_until_idle(
    idle: Duration,
    stop: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    let snap = peer::snapshot_self().ok_or_else(|| anyhow::anyhow!("hub image inode"))?;
    let want = peer::Identity::of_self(snap.inode());
    super::guard::lock_memory();
    let path = socket_path();
    let lock = lock_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = match UnixListener::bind(&path) {
        Ok(listener) => listener,
        Err(_) => return Ok(()),
    };
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    let bound = peer::inode_of(&path).ok_or_else(|| anyhow::anyhow!("hub socket inode"))?;
    let mut tables = Tables::new(crate::global_config::GlobalConfig::path());
    let mut last_request = now();
    tokio::pin!(stop);
    loop {
        tokio::select! {
            biased;
            () = &mut stop => break,
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                last_request = now();
                let conn = handle_conn(&mut tables, stream, &want);
                let after = tokio::time::timeout(CONN_TIMEOUT, conn)
                    .await
                    .unwrap_or(AfterConn::Continue);
                if let Some(ledger) = tables.release.take_version_fetch() {
                    spawn_version_fetch(ledger);
                }
                if matches!(after, AfterConn::Quit) {
                    break;
                }
            }
            () = tokio::time::sleep(nap(&tables, idle)) => {
                let reachable =
                    peer::inode_of(&path) == Some(bound) && peer::image_is_current(want.inode);
                if should_exit(&mut tables, reachable, since(last_request) >= idle) {
                    break;
                }
            }
        }
    }
    drop(listener);
    // A client that saw this hub as slow or stale may have unlinked the
    // path and bound a fresh hub there. Never remove that one's socket.
    // The next spawn recreates the lock. Two spawners on two lock files
    // only race the exclusive bind.
    if peer::inode_of(&path) == Some(bound) {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&lock);
    }
    Ok(())
}

/// Tunnels need their leases checked and their ttl counted down.
fn nap(tables: &Tables, idle: Duration) -> Duration {
    if tables.tunnels.is_empty() {
        idle.min(REACH_CHECK)
    } else {
        idle.min(TUNNEL_TICK)
    }
}

/// An orphan exits even with live rows: its socket path is gone or belongs
/// to another hub, or a reinstall replaced its binary, so new clients use
/// another socket. Otherwise only an idle hub with empty tables exits.
/// Prunes on every nap: a busy hub still drops dead leases and expires
/// tunnels without waiting for a request.
fn should_exit(tables: &mut Tables, reachable: bool, idle: bool) -> bool {
    let empty = tables.is_empty();
    !reachable || (idle && empty)
}

async fn handle_conn(
    tables: &mut Tables,
    mut stream: UnixStream,
    want: &peer::Identity,
) -> AfterConn {
    if !peer::peer_allowed(&stream, want) {
        send_denied(&mut stream).await;
        return AfterConn::Continue;
    }
    match read_req(&mut stream).await {
        Some(wire::InReq::Body(req)) => {
            let quit = matches!(req, Req::Quit);
            let Some(frame) = wire::encode_rep(&tables.handle(req)) else {
                return AfterConn::Continue;
            };
            let _ = stream.write_all(&frame).await;
            if quit {
                AfterConn::Quit
            } else {
                AfterConn::Continue
            }
        }
        Some(wire::InReq::WrongVer) => {
            send_denied(&mut stream).await;
            AfterConn::Continue
        }
        None => AfterConn::Continue,
    }
}

async fn send_denied(stream: &mut UnixStream) {
    if let Some(frame) = wire::encode_rep(&Rep::Err(ERR_DENIED)) {
        let _ = stream.write_all(&frame).await;
    }
}

async fn read_req(stream: &mut UnixStream) -> Option<wire::InReq> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header).await.ok()?;
    let n = wire::header_len(&header)?;
    let mut body = vec![0u8; n as usize];
    stream.read_exact(&mut body).await.ok()?;
    let mut frame = Vec::with_capacity(8 + body.len());
    frame.extend_from_slice(&header);
    frame.extend_from_slice(&body);
    wire::decode_in_req(&frame)
}

pub fn socket_path() -> PathBuf {
    crate::cache::root().join(format!("hub-{}.sock", peer::instance_token()))
}

pub fn lock_path() -> PathBuf {
    crate::cache::root().join(format!("hub-{}.lock", peer::instance_token()))
}

#[cfg(test)]
pub(crate) fn should_exit_for_test(tables: &mut Tables, reachable: bool, idle: bool) -> bool {
    should_exit(tables, reachable, idle)
}
