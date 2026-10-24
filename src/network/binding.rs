//! One tunnel binding, reused or started in the hub. Every tunnel lives
//! there, so one place knows every Lade tunnel on a port.

use anyhow::{Result, anyhow, bail};
use lade_sdk::network::{ensure_provider_preflight, kube_context};
use std::os::unix::ffi::OsStrExt;
use std::time::{Duration, Instant};

use crate::config::{NetworkBinding, RuleTtl, tunnel_ttl_ms};
use crate::hub::{HubState, Lease, TunnelAsk, TunnelLaunch, TunnelRep};
use crate::network::Stop;
use crate::network::labels::{
    PORT_WAIT_MINUTES, connection_label, env_entry_for, port_held, port_wait_message, send_failed,
    send_progress, target_label,
};
use crate::network::parse::{parse_binding, reconcile_local_port};
use crate::network::progress::ProviderProgressKind;
use crate::network::types::ParsedBinding;
use crate::provider_progress::ProviderProgressSink;

pub(super) const HUB_POLL: Duration = Duration::from_millis(50);
// Past the supervisor's 60 s startup budget, so its error wins.
const HUB_START_TIMEOUT: Duration = Duration::from_secs(70);
const HUB_SILENCE: Duration = Duration::from_secs(2);

pub(super) struct Acquired {
    pub(super) env_entry: Option<(String, String)>,
    /// What the hub holds for this binding, to hand to the shell.
    pub(super) ask: TunnelAsk,
}

struct Opened {
    port: u16,
    pid: u32,
    reused: bool,
    ask: TunnelAsk,
}

const PORT_WAIT: Duration = Duration::from_secs(PORT_WAIT_MINUTES * 60);
const PORT_POLL: Duration = Duration::from_millis(500);

pub(super) fn acquire_binding(
    binding: NetworkBinding,
    lease: Lease,
    session: Option<RuleTtl>,
    progress: ProviderProgressSink,
    stop: &Stop,
) -> Result<Acquired> {
    let started = Instant::now();
    let parsed = parse_binding(&binding)?;
    let port = reconcile_local_port(&parsed.target, parsed.local_port)?;
    let progress_id = format!("{}|{}", binding.key, binding.uri);
    let pending = match port {
        Some(port) => connection_label(&parsed.spec, &parsed.local_host, port),
        None => target_label(&parsed.spec),
    };
    let show = |label: String| {
        send_progress(
            &progress,
            &progress_id,
            label,
            ProviderProgressKind::Connecting,
        )
    };
    show(pending.clone());
    let ttl_ms = tunnel_ttl_ms(binding.ttl.as_ref(), session.as_ref());
    let outcome = via_hub(
        &binding, &parsed, port, &pending, ttl_ms, &lease, stop, &show,
    )
    .unwrap_or_else(|| {
        Err(anyhow!(
            "Tunnels run in the Lade hub, and it is {}. {pending} was not started.",
            hub_state()
        ))
    });
    let opened = match outcome {
        Ok(opened) => opened,
        Err(e) => {
            send_failed(&progress, progress_id, pending, started);
            return Err(e);
        }
    };
    let reused = if opened.reused { " reused" } else { "" };
    let connected = format!(
        "{} pid={}{reused} {} ms",
        connection_label(&parsed.spec, &parsed.local_host, opened.port),
        opened.pid,
        started.elapsed().as_millis()
    );
    send_progress(
        &progress,
        &progress_id,
        connected,
        ProviderProgressKind::Connected,
    );
    Ok(Acquired {
        env_entry: env_entry_for(&parsed.target, opened.port),
        ask: opened.ask,
    })
}

/// Reuses or starts the tunnel in the hub. While another Lade tunnel holds
/// the port, waits up to [`PORT_WAIT`] for it to go. `None`: the hub did
/// not answer.
#[allow(clippy::too_many_arguments)]
fn via_hub(
    binding: &NetworkBinding,
    parsed: &ParsedBinding,
    port: Option<u16>,
    pending: &str,
    ttl_ms: Option<u32>,
    lease: &Lease,
    stop: &Stop,
    show: &dyn Fn(String),
) -> Option<Result<Opened>> {
    let mut ask = TunnelAsk {
        key: binding.key.clone(),
        uri: binding.uri.clone(),
        host: parsed.local_host.clone(),
        port,
        fingerprint: format!("{}|{}", parsed.local_host, binding.uri),
        ttl_ms,
        lease: lease.clone(),
        launch: None,
    };
    let waiting_since = Instant::now();
    let mut waited = false;
    let mut rep = crate::hub::tunnel_open(ask.clone())?;
    loop {
        match rep {
            TunnelRep::Absent if ask.launch.is_none() => {
                match launch_for(parsed, stop) {
                    Ok(launch) => ask.launch = Some(launch),
                    Err(e) => return Some(Err(e)),
                }
                rep = crate::hub::tunnel_open(ask.clone())?;
            }
            TunnelRep::Conflict {
                port,
                holder: Some((ref key, ref uri)),
            } if waiting_since.elapsed() < PORT_WAIT => {
                show(port_wait_message(port, pending, key, uri));
                waited = true;
                if stop.sleep(PORT_POLL) {
                    return Some(Err(anyhow!(
                        "{pending} was stopped while it waited for port {port}"
                    )));
                }
                rep = crate::hub::tunnel_open(ask.clone())?;
            }
            rep => {
                if waited {
                    show(pending.to_string());
                }
                ask.launch = None;
                return Some(
                    settle(rep, pending, stop).map(|(port, pid, reused)| Opened {
                        port,
                        pid,
                        reused,
                        ask,
                    }),
                );
            }
        }
    }
}
/// Preflight runs here, where the terminal is: `tsh kube login` may prompt.
fn launch_for(parsed: &ParsedBinding, stop: &Stop) -> Result<TunnelLaunch> {
    match &parsed.spec {
        crate::network::types::ProviderSpec::TshKubeCluster {
            teleport_proxy,
            kube_cluster,
            ..
        } => login_tsh(teleport_proxy, kube_cluster, stop)?,
        spec => ensure_provider_preflight(spec)?,
    }
    Ok(TunnelLaunch {
        context: kube_context(&parsed.spec)?,
        env: std::env::vars_os()
            .map(|(key, value)| (key.as_bytes().to_vec(), value.as_bytes().to_vec()))
            .collect(),
    })
}

fn login_tsh(proxy: &str, cluster: &str, stop: &Stop) -> Result<()> {
    let mut child = std::process::Command::new("tsh")
        .arg(format!("--proxy={proxy}"))
        .arg("kube")
        .arg("login")
        .arg(cluster)
        .spawn()?;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => bail!("tsh kube login failed for cluster '{cluster}'"),
            Ok(None) => {
                if stop.sleep(HUB_POLL) {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("stopped while logging in to the tunnel provider");
                }
            }
            Err(e) => return Err(e.into()),
        }
    }
}

/// Polls a hub tunnel until it is ready. Returns its port, pid, and whether
/// it was reused.
fn settle(mut rep: TunnelRep, pending: &str, stop: &Stop) -> Result<(u16, u32, bool)> {
    let started = Instant::now();
    let mut silent_since: Option<Instant> = None;
    loop {
        match rep {
            TunnelRep::Ready {
                port, pid, reused, ..
            } => return Ok((port, pid, reused)),
            TunnelRep::Starting { id, .. } => {
                if started.elapsed() >= HUB_START_TIMEOUT {
                    bail!("the Lade hub did not start {pending} in time");
                }
                if stop.sleep(HUB_POLL) {
                    bail!("{pending} was stopped while it started");
                }
                match crate::hub::tunnel_poll(id) {
                    Some(next) => {
                        silent_since = None;
                        rep = next;
                    }
                    None => {
                        if silent_since.get_or_insert_with(Instant::now).elapsed() >= HUB_SILENCE {
                            bail!("the Lade hub stopped answering while {pending} started");
                        }
                    }
                }
            }
            TunnelRep::Failed { error } => bail!("{error}"),
            TunnelRep::Conflict { port, holder } => {
                return Err(port_held(
                    port,
                    pending,
                    holder
                        .as_ref()
                        .map(|(key, uri)| (key.as_str(), uri.as_str())),
                ));
            }
            TunnelRep::Absent => bail!("the Lade hub lost {pending} while it started"),
        }
    }
}

fn hub_state() -> &'static str {
    match crate::hub::probe().state {
        HubState::Stale => "stale (another Lade binary)",
        HubState::Down => "down",
        HubState::Up => "not answering",
    }
}
