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
    serve_until_idle(IDLE_EXIT, REACH_CHECK, stop).await
}

pub(super) async fn serve_until_idle(
    idle: Duration,
    reach: Duration,
    stop: impl Future<Output = ()>,
) -> anyhow::Result<()> {
    let snap = peer::snapshot_self().ok_or_else(|| anyhow::anyhow!("hub image inode"))?;
    let want = peer::Identity::of_self(snap.inode());
    // § hxw00jp6v.a@7ss1m
    let path = socket_path().ok_or_else(|| anyhow::anyhow!(peer::NO_IMAGE))?;
    let lock = lock_path().ok_or_else(|| anyhow::anyhow!(peer::NO_IMAGE))?;
    // § end hxw00jp6v.a
    // § one kgejwr97c.a@6azsp
    let unguarded = super::guard::lock_memory().err().map(|why| why.to_string());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // § hxw00jp6v.c@c45fz
    let listener = match bind_owner_only(&path) {
        Ok(listener) => listener,
        Err(_) => return Ok(()),
    };
    // § end hxw00jp6v.c
    // § hxw00jp6v.d@nkgej
    if let Err(e) = make_owner_only(&path) {
        drop(listener);
        let _ = std::fs::remove_file(&path);
        return Err(e);
    }
    // § end hxw00jp6v.d
    let bound = peer::inode_of(&path).ok_or_else(|| anyhow::anyhow!("hub socket inode"))?;
    // § one cbz2he75h.d@fy2dj
    let mut tables = Tables::new(crate::global_config::GlobalConfig::path(), unguarded);
    let mut last_request = now();
    // § one cbz2he75h.b@m7q9j
    // Requests do not move this clock: a stream of them from old clients
    // must not keep an orphan up.
    let mut next_check = tokio::time::Instant::now() + nap(&tables, idle, reach);
    tokio::pin!(stop);
    loop {
        tokio::select! {
            biased;
            () = &mut stop => break,
            () = tokio::time::sleep_until(next_check) => {
                // § cbz2he75h.b@v5jgq
                let reachable =
                    peer::inode_of(&path) == Some(bound) && peer::image_is_current(want.inode);
                if should_exit(&mut tables, reachable, since(last_request) >= idle) {
                    break;
                }
                next_check = tokio::time::Instant::now() + nap(&tables, idle, reach);
                // § end cbz2he75h.b
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                last_request = now();
                let conn = handle_conn(&mut tables, stream, &want);
                // § dd89rj943.c@crp06
                let after = tokio::time::timeout(CONN_TIMEOUT, conn)
                    .await
                    .unwrap_or(AfterConn::Continue);
                // § end dd89rj943.c
                if let Some(ledger) = tables.release.take_version_fetch() {
                    // § d1d316x4b.b@ax29b
                    spawn_version_fetch(ledger);
                    // § end d1d316x4b.b
                }
                if matches!(after, AfterConn::Quit) {
                    break;
                }
                // A tunnel opened by this request needs its leases checked
                // sooner than an empty hub does.
                next_check = next_check.min(tokio::time::Instant::now() + nap(&tables, idle, reach));
            }
        }
    }
    drop(listener);
    // A client that saw this hub as slow or stale may have unlinked the
    // path and bound a fresh hub there. Never remove that one's socket.
    // The next spawn recreates the lock. Two spawners on two lock files
    // only race the exclusive bind.
    if peer::inode_of(&path) == Some(bound) {
        // § cbz2he75h.c@whszs
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&lock);
    }
    Ok(())
}

/// The socket never exists with the umask's mode, which may let another
/// user connect before a `chmod`. The umask is restored at once: in tests
/// the hub shares its process.
fn bind_owner_only(path: &std::path::Path) -> std::io::Result<UnixListener> {
    // § hxw00jp6v.d@tcrn1
    let old = unsafe { libc::umask(0o077) };
    let bound = UnixListener::bind(path);
    unsafe { libc::umask(old) };
    bound
}

fn make_owner_only(path: &std::path::Path) -> anyhow::Result<()> {
    // § hxw00jp6v.d@gvfc7
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| anyhow::anyhow!("hub socket {}: {e}", path.display()))?;
    let mode = std::fs::metadata(path)?.permissions().mode() & 0o777;
    if mode != 0o600 {
        anyhow::bail!(
            "hub socket {} has mode {mode:o}, not 600: the hub does not serve",
            path.display()
        );
    }
    Ok(())
}

/// Tunnels need their leases checked and their ttl counted down.
fn nap(tables: &Tables, idle: Duration, reach: Duration) -> Duration {
    // § cbz2he75h.b@cm7cj
    if tables.tunnels.is_empty() {
        idle.min(reach)
    } else {
        // § gv81k6nf4.c@t9eyz
        idle.min(TUNNEL_TICK)
    }
}

/// An orphan exits even with live rows: its socket path is gone or belongs
/// to another hub, or a reinstall replaced its binary, so new clients use
/// another socket. Otherwise only an idle hub with empty tables exits.
/// Prunes on every nap: a busy hub still drops dead leases and expires
/// tunnels without waiting for a request.
fn should_exit(tables: &mut Tables, reachable: bool, idle: bool) -> bool {
    // § cbz2he75h.a@k92gm
    let empty = tables.is_empty();
    !reachable || (idle && empty)
}

async fn handle_conn(
    tables: &mut Tables,
    mut stream: UnixStream,
    want: &peer::Identity,
) -> AfterConn {
    if !peer::peer_allowed(&stream, want) {
        // § e1s6kxps0.d@g6taj
        send_denied(&mut stream).await;
        return AfterConn::Continue;
    }
    match read_req(&mut stream).await {
        Some(wire::InReq::Body(req)) => {
            let quit = matches!(*req, Req::Quit);
            let Some(frame) = wire::encode_rep(&tables.handle(*req)) else {
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
            // § dd89rj943.b@sjx3g
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

/// `None`: this image cannot read its own file, so it has no hub.
pub fn socket_path() -> Option<PathBuf> {
    // § hxw00jp6v.a@90yce
    Some(crate::cache::root().join(format!("hub-{}.sock", peer::instance_token()?)))
}

pub fn lock_path() -> Option<PathBuf> {
    // § hxw00jp6v.c@fw3fd
    Some(crate::cache::root().join(format!("hub-{}.lock", peer::instance_token()?)))
}

#[cfg(test)]
pub(crate) fn should_exit_for_test(tables: &mut Tables, reachable: bool, idle: bool) -> bool {
    should_exit(tables, reachable, idle)
}
