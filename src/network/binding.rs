//! One tunnel binding, reused or started in the hub. Every tunnel lives
//! there, so one place knows every Lade tunnel on a port.

use anyhow::{Result, anyhow, bail};
use lade_sdk::network::{ensure_provider_preflight, kube_context, tsh_kube_login};
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
pub(crate) const HUB_START_TIMEOUT: Duration = Duration::from_secs(70);
// § one v29epeav2.d@brkb7
const _: () =
    assert!(HUB_START_TIMEOUT.as_millis() > crate::network::process::STARTUP_TIMEOUT.as_millis());
/// Past the remote supervisor's budget for a tunnel through the laptop.
pub(crate) const LINKED_HUB_START_TIMEOUT: Duration = Duration::from_secs(110);
const _: () = assert!(
    LINKED_HUB_START_TIMEOUT.as_millis()
        > crate::network::process::LINKED_STARTUP_TIMEOUT.as_millis()
);
const HUB_SILENCE: Duration = Duration::from_secs(2);

pub(super) struct Acquired {
    pub(super) port: u16,
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

// § one vpxx2dd6c.c@83mn9
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
    // § r14dqggh2@09f4b
    let outcome = via_hub(
        &binding, &parsed, port, &pending, ttl_ms, &lease, stop, &show, PORT_WAIT,
    )
    .unwrap_or_else(|| {
        Err(anyhow!(
            "Tunnels run in the Lade hub, and it is {}. {pending} was not started.",
            hub_state()
        ))
    });
    // § end r14dqggh2
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
        port: opened.port,
        env_entry: env_entry_for(&parsed.target, opened.port),
        ask: opened.ask,
    })
}

/// Reuses or starts the tunnel in the hub. While another Lade tunnel holds
/// the port, waits up to `port_wait` for it to go, [`PORT_WAIT`] but in
/// tests. `None`: the hub did not answer.
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
    port_wait: Duration,
) -> Option<Result<Opened>> {
    // Every command asks the laptop, a reuse too: an open tunnel says
    // nothing about this command's walk.
    let link_walk = match crate::remote::tunnel::check(&parsed.spec, &binding.uri) {
        Ok(walk) => walk,
        Err(e) => return Some(Err(e)),
    };
    // A tunnel on the link and one with this machine's credentials are two
    // targets: neither is reused for the other.
    let through = if link_walk.is_some() { "link|" } else { "" };
    let start_timeout = if link_walk.is_some() {
        LINKED_HUB_START_TIMEOUT
    } else {
        HUB_START_TIMEOUT
    };
    // § gab93p2gf.a@rtg91
    let mut ask = TunnelAsk {
        key: binding.key.clone(),
        uri: binding.uri.clone(),
        host: parsed.local_host.clone(),
        port,
        fingerprint: format!("{through}{}|{}", parsed.local_host, binding.uri),
        ttl_ms,
        lease: lease.clone(),
        launch: None,
    };
    // § end gab93p2gf.a
    let waiting_since = Instant::now();
    let mut waited = false;
    let mut rep = crate::hub::tunnel_open(ask.clone())?;
    loop {
        match rep {
            TunnelRep::Absent if ask.launch.is_none() => {
                match launch_for(link_walk.clone(), parsed, stop) {
                    Ok(launch) => ask.launch = Some(launch),
                    Err(e) => return Some(Err(e)),
                }
                rep = crate::hub::tunnel_open(ask.clone())?;
            }
            // § one vpxx2dd6c.c@dedhq, ewd09vh5j.c@3pj1m
            TunnelRep::Conflict {
                port,
                holder: Some((ref key, ref uri)),
            } if waiting_since.elapsed() < port_wait => {
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
                return Some(settle(rep, pending, start_timeout, stop).map(
                    |(port, pid, reused)| Opened {
                        port,
                        pid,
                        reused,
                        ask,
                    },
                ));
            }
        }
    }
}
/// A tunnel this laptop's hub runs for an attached remote, under `lease`.
/// Its key is its own and `local=` is dropped, so it never takes a port or
/// a tunnel of the laptop's own commands. Returns the laptop port.
pub(crate) fn open_for_remote(host: &str, uri: &str, lease: Lease) -> Result<u16> {
    // § aswq5qe7f.c@m6g0g
    use sha2::Digest;
    let mut url = url::Url::parse(uri)?;
    let query = url
        .query_pairs()
        .filter(|(key, _)| key != "local")
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();
    url.set_query(None);
    if !query.is_empty() {
        url.query_pairs_mut().extend_pairs(query);
    }
    let digest = sha2::Sha256::digest(format!("{host}\u{0}{uri}").as_bytes());
    let key = format!("LADE_REMOTE_{}", &crate::remote::hex(&digest)[..12]);
    let binding = NetworkBinding {
        key,
        uri: url.to_string(),
        ttl: None,
    };
    let acquired = acquire_binding(
        binding,
        lease,
        None,
        ProviderProgressSink::silent(),
        &Stop::default(),
    )?;
    Ok(acquired.port)
}

/// Preflight runs here, where the terminal is: `tsh kube login` may prompt.
/// On an attached remote the provider is the link: no local credential or
/// CLI is read.
fn launch_for(
    link_walk: Option<String>,
    parsed: &ParsedBinding,
    stop: &Stop,
) -> Result<TunnelLaunch> {
    let env = std::env::vars_os()
        .map(|(key, value)| (key.as_bytes().to_vec(), value.as_bytes().to_vec()))
        .collect::<Vec<_>>();
    if link_walk.is_some() {
        return Ok(TunnelLaunch {
            context: None,
            env,
            link_walk,
        });
    }
    // § n6qm5smv5.a@pdtgz
    match &parsed.spec {
        crate::network::types::ProviderSpec::TshKubeCluster {
            teleport_proxy,
            kube_cluster,
            ..
        } => tsh_kube_login(teleport_proxy, kube_cluster, || stop.is_stopped())?,
        spec => ensure_provider_preflight(spec)?,
    }
    // § end n6qm5smv5.a
    // § n6qm5smv5.c@8fmtp
    Ok(TunnelLaunch {
        context: kube_context(&parsed.spec)?,
        env,
        link_walk: None,
    })
}

/// Polls a hub tunnel until it is ready. Returns its port, pid, and whether
/// it was reused.
fn settle(
    mut rep: TunnelRep,
    pending: &str,
    timeout: Duration,
    stop: &Stop,
) -> Result<(u16, u32, bool)> {
    let started = Instant::now();
    let mut silent_since: Option<Instant> = None;
    loop {
        match rep {
            TunnelRep::Ready {
                port, pid, reused, ..
            } => return Ok((port, pid, reused)),
            TunnelRep::Starting { id, .. } => {
                if started.elapsed() >= timeout {
                    // § v29epeav2.d@724cb
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
                // § ewd09vh5j.c@rmqyv
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

fn hub_state() -> String {
    if !crate::hub::has_image() {
        return format!("unreachable: {}", crate::hub::NO_IMAGE);
    }
    match crate::hub::probe().state {
        HubState::Stale => "stale (another Lade binary)",
        HubState::Down => "down",
        HubState::Up => "not answering",
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    #[test]
    fn a_port_held_by_another_lade_tunnel_names_it_and_how_to_free_it() {
        let error = port_held(
            15432,
            "kubectl forward stg",
            Some(("15432", "kubectl://k/prd/default/service/db/5432")),
        )
        .to_string();
        assert!(
            error.contains("kubectl://k/prd/default/service/db/5432"),
            "{error}"
        );
        assert!(error.contains("`lade ttl forget 15432`"), "{error}");
        assert!(
            error.contains(&format!("{PORT_WAIT_MINUTES} min")),
            "{error}"
        );
    }

    fn binding(port: u16, cluster: &str) -> NetworkBinding {
        NetworkBinding {
            key: port.to_string(),
            uri: format!("kubectl://kube.example.com:443/{cluster}/default/service/db/5432"),
            ttl: None,
        }
    }

    fn own_lease(id: &str) -> Lease {
        let pid = std::process::id();
        Lease {
            id: id.to_string(),
            pid,
            started: crate::network::process_started(pid).expect("own start time"),
        }
    }

    /// A hub in this process holds a starting tunnel to prd on a fixed
    /// port; a command for stg on that port waits, then gives up.
    #[test]
    fn a_held_port_gives_up_after_the_wait_naming_the_holder() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("cache");
        let bin = root.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let kubectl = bin.join("kubectl");
        std::fs::write(&kubectl, "#!/bin/sh\nexec sleep 60\n").unwrap();
        std::fs::set_permissions(&kubectl, std::fs::Permissions::from_mode(0o755)).unwrap();
        let cache_s = cache.to_string_lossy().into_owned();
        temp_env::with_vars([("LADE_CACHE_DIR", Some(cache_s.as_str()))], || {
            std::thread::spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                let _ = rt.block_on(crate::hub::serve(std::future::pending()));
            });
            let deadline = Instant::now() + Duration::from_secs(5);
            while crate::hub::probe().state != HubState::Up {
                assert!(Instant::now() < deadline, "hub never came up");
                std::thread::sleep(Duration::from_millis(10));
            }
            let port = crate::network::free_fixed_port();
            let prd = binding(port, "prd");
            let holder = crate::hub::tunnel_open(TunnelAsk {
                key: prd.key.clone(),
                uri: prd.uri.clone(),
                host: "127.0.0.1".to_string(),
                port: Some(port),
                fingerprint: format!("127.0.0.1|{}", prd.uri),
                ttl_ms: None,
                lease: own_lease("holder"),
                launch: Some(TunnelLaunch {
                    context: Some("prd".to_string()),
                    env: vec![(
                        b"PATH".to_vec(),
                        format!("{}:/bin:/usr/bin", bin.display()).into_bytes(),
                    )],
                    link_walk: None,
                }),
            });
            assert!(
                matches!(holder, Some(TunnelRep::Starting { .. })),
                "{holder:?}"
            );

            let stg = binding(port, "stg");
            let parsed = parse_binding(&stg).unwrap();
            let waited = std::sync::Mutex::new(Vec::new());
            let show = |line: String| waited.lock().unwrap().push(line);
            let wait = Duration::from_millis(300);
            let started = Instant::now();
            let outcome = via_hub(
                &stg,
                &parsed,
                Some(port),
                "kubectl forward stg",
                None,
                &own_lease("stg"),
                &Stop::default(),
                &show,
                wait,
            );
            let took = started.elapsed();
            crate::hub::tunnel_release("holder");
            assert_eq!(crate::hub::stop(), crate::hub::HubStop::Stopped);

            let error = match outcome {
                Some(Err(error)) => error.to_string(),
                Some(Ok(_)) => panic!("stg reached the port prd holds"),
                None => panic!("the hub did not answer"),
            };
            assert!(took >= wait && took < Duration::from_secs(5), "{took:?}");
            assert!(error.contains(&prd.uri), "{error}");
            assert!(
                error.contains(&format!("`lade ttl forget {port}`")),
                "{error}"
            );
            assert!(
                waited
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|line| line.contains("Waiting up to")),
                "no wait notice"
            );
        });
    }
}
