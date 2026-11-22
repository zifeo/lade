//! Tunnels on an attached remote: the remote hub keeps the port, its
//! leases and its restarts, and `lade __linked-tunnel` is the provider. It
//! listens where the hub relays each connection and carries every one of
//! them over the link to a tunnel the laptop hub runs.

use anyhow::{Context, Result, bail};
use lade_sdk::network::ProviderSpec;
use lade_sdk::upstream::{self, Attached, Frame, read_frame, write_frame};
use std::io::Write;
use std::path::Path;
use tokio::io::AsyncReadExt;
use tokio::net::UnixListener;

fn attached() -> Option<Attached> {
    lade_sdk::upstream::attached()
}

/// `kubefwd` binds addresses on the machine that runs the command, so it
/// stays here with that machine's credentials.
fn linked(spec: &ProviderSpec) -> bool {
    !matches!(spec, ProviderSpec::Kubefwd { .. })
}

/// On an attached remote, asks the laptop whether it opens `uri` for this
/// command's walk, and returns that walk: exit 4, 5 or 6 come from here,
/// before the hub starts anything. `None` off a link.
pub(crate) fn check(spec: &ProviderSpec, uri: &str) -> Result<Option<String>> {
    let Some(attached) = attached() else {
        return Ok(None);
    };
    if !linked(spec) {
        return Ok(None);
    }
    let walk = (attached.walk)()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let reply = runtime.block_on(open(&attached, &walk, uri, true))?;
    drop(reply);
    Ok(Some(walk))
}

/// Opens `uri` on the laptop. `check`: only asks.
async fn open(
    attached: &Attached,
    walk: &str,
    uri: &str,
    check: bool,
) -> Result<(u64, tokio::net::UnixStream)> {
    let named = [uri.to_string()];
    let mut stream = upstream::session(attached, &named).await?;
    write_frame(
        &mut stream,
        &Frame::TunnelOpen {
            id: 0,
            walk: walk.to_string(),
            uri: uri.to_string(),
            check,
            report: (attached.report)(),
        },
    )
    .await?;
    match read_frame(&mut stream).await? {
        Some(Frame::TunnelReady { id }) => Ok((id, stream)),
        Some(Frame::Failed { code, message, .. }) => Err(upstream::failed(attached, code, message)),
        _ => Err(upstream::absent(
            attached,
            &named,
            "the link closed before the laptop answered",
        )),
    }
}

/// `lade __linked-tunnel`: run by the remote hub as the tunnel's provider.
/// Exits once the laptop tunnel or the link is gone, and the hub restarts
/// it as it restarts any provider.
pub(crate) async fn run(uri: &str, walk: &str, listen: &Path) -> Result<i32> {
    // § aswq5qe7f@kt3tr
    let Some(attached) = attached() else {
        bail!("This machine is not attached to a laptop: {uri} has no provider here.");
    };
    let (tunnel, mut control) = match open(&attached, walk, uri, false).await {
        Ok(opened) => opened,
        Err(e) => {
            // § aswq5qe7f.b@zkyr8
            // The laptop already retried: a retry here would start a fresh,
            // silent log that the hub's budget ends with, losing this error.
            // Pending, this child's log is what the budget reports.
            let _ = writeln!(std::io::stderr(), "{e:#}");
            std::future::pending::<()>().await;
            unreachable!()
        }
    };
    let _ = std::fs::remove_file(listen);
    let listener = UnixListener::bind(listen)
        .with_context(|| format!("cannot listen on {}", listen.display()))?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(listen, std::fs::Permissions::from_mode(0o600))?;
    }
    let mut byte = [0u8; 1];
    loop {
        tokio::select! {
            // The control connection ends with the link or the laptop tunnel.
            _ = control.read(&mut byte) => {
                let _ = writeln!(std::io::stderr(), "the link to {} closed", attached.laptop);
                return Ok(crate::exit_codes::FAILURE);
            }
            accepted = listener.accept() => {
                let (socket, _) = accepted?;
                let attached = attached.clone();
                tokio::spawn(async move {
                    let _ = carry(&attached, tunnel, socket).await;
                });
            }
        }
    }
}

/// One connection, carried over the link as a stream of `tunnel`.
async fn carry(attached: &Attached, tunnel: u64, mut socket: tokio::net::UnixStream) -> Result<()> {
    let mut stream = upstream::session(attached, &[]).await?;
    write_frame(&mut stream, &Frame::StreamOpen { id: 0, tunnel }).await?;
    match read_frame(&mut stream).await? {
        Some(Frame::StreamReady { .. }) => {}
        Some(Frame::Failed { message, .. }) => bail!(message),
        _ => bail!("the link closed before the laptop answered"),
    }
    tokio::io::copy_bidirectional(&mut socket, &mut stream).await?;
    Ok(())
}
