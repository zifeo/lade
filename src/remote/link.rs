//! `lade __remote`, run by the laptop hub: the transport to `lade attach`,
//! and every request it carries answered from the laptop's own resolution of
//! the walks `lade connect` resolved.

use anyhow::{Context, Result};
use lade_sdk::upstream::{self, Frame, read_frame, skip_to_magic, write_frame};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::Write;
use std::process::Stdio;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::io::AsyncBufReadExt;
use tokio::sync::mpsc;

use super::resolve::ResolvedWalk;
use super::streams::{Out, Streams};
use super::transport::{Kind, Transport};
use crate::audience::Via;
use crate::config::Audience;
use crate::event::{self, Emit, Kind as EventKind};
use crate::hub::{
    ATTEMPT_ENV, LIFELINE_DETACH, LIFELINE_WALK, LINK_DETACHED, LINK_GIVE_UP, LINK_READY,
    LINK_WALK, SPEC_ENV,
};

const PING_EVERY: Duration = Duration::from_secs(15);
const PONG_LOST: Duration = Duration::from_secs(45);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(60);
/// A transport error a retry cannot clear: a human must sign in.
// § dzex6kwmq.a@y5kpw
const NEEDS_HUMAN: &[&str] = &[
    "Permission denied",
    "Host key verification failed",
    "REMOTE HOST IDENTIFICATION HAS CHANGED",
    "passphrase",
    "tsh login",
    "command not found",
    "not found in PATH",
];
// § end dzex6kwmq.a

/// How `__remote` reaches the remote, handed to it through the hub. The
/// walks come on the lifeline.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct LinkSpec {
    pub transport: Vec<String>,
    /// The transport as the caller wrote it after `--`.
    pub written: String,
    pub host: String,
    pub laptop: String,
    pub control_path: Option<String>,
}

/// The walks this link serves, by their hex hash.
type Walks = Arc<RwLock<BTreeMap<String, Arc<ResolvedWalk>>>>;

/// The remote command: `lade attach` with what its marker records.
pub(crate) fn attach_command(spec: &LinkSpec, resume: bool) -> String {
    use base64::Engine;
    let engine = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    format!(
        "lade attach --laptop {} --transport {}{}",
        engine.encode(&spec.laptop),
        engine.encode(&spec.written),
        if resume { " --resume" } else { "" }
    )
}

pub(crate) fn decode_arg(raw: &str) -> Result<String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .context("lade attach takes the arguments lade connect wrote")?;
    Ok(String::from_utf8(bytes)?)
}

fn give_up(message: impl std::fmt::Display) -> i32 {
    let _ = writeln!(std::io::stderr(), "{message}");
    LINK_GIVE_UP
}

fn drop_link(message: impl std::fmt::Display) -> i32 {
    let _ = writeln!(std::io::stderr(), "{message}");
    crate::exit_codes::FAILURE
}

pub(crate) async fn run() -> Result<i32> {
    let spec: LinkSpec = serde_json::from_str(
        &std::env::var(SPEC_ENV).with_context(|| format!("lade __remote needs {SPEC_ENV}"))?,
    )?;
    let attempt: u32 = std::env::var(ATTEMPT_ENV)
        .with_context(|| format!("lade __remote needs {ATTEMPT_ENV}"))?
        .parse()
        .with_context(|| format!("{ATTEMPT_ENV} is not a number"))?;
    let walks = Walks::default();
    let (hub_tx, mut hub_rx) = mpsc::unbounded_channel::<bool>();
    tokio::spawn(lifeline(Arc::clone(&walks), hub_tx));
    let transport = Transport::parse(&spec.transport)?;
    let mut options = vec![
        "BatchMode=yes".to_string(),
        "ServerAliveInterval=15".to_string(),
        "ServerAliveCountMax=3".to_string(),
    ];
    if let Some(path) = &spec.control_path {
        options.push("ControlMaster=no".to_string());
        options.push(format!("ControlPath={path}"));
    }
    let mut command = tokio::process::Command::from(
        transport.command(&options, &attach_command(&spec, attempt > 0)),
    );
    command
        .env_remove(SPEC_ENV)
        .env_remove(ATTEMPT_ENV)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .with_context(|| format!("cannot run {}", spec.transport[0]))?;
    let mut to_remote = child.stdin.take().context("transport stdin")?;
    let mut from_remote = child.stdout.take().context("transport stdout")?;
    let stderr = child.stderr.take().context("transport stderr")?;
    let errors = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    {
        let errors = Arc::clone(&errors);
        tokio::spawn(async move {
            let mut lines = tokio::io::BufReader::new(stderr).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut errors = errors.lock().unwrap_or_else(|e| e.into_inner());
                errors.push(super::shown(&line));
                let over = errors.len().saturating_sub(20);
                errors.drain(..over);
            }
        });
    }
    let failed = |why: String| {
        let tail = errors.lock().unwrap_or_else(|e| e.into_inner()).join("\n");
        let message = if tail.is_empty() {
            why
        } else {
            format!("{why}: {}", tail.trim())
        };
        if NEEDS_HUMAN.iter().any(|pattern| message.contains(pattern)) {
            give_up(match transport.kind {
                Kind::Ssh => {
                    format!("{message}. ssh needs a human: run lade connect in a terminal.")
                }
                Kind::Tsh => format!("{message}. tsh login required."),
            })
        } else {
            drop_link(message)
        }
    };

    let handshake = async {
        skip_to_magic(&mut from_remote).await?;
        read_frame(&mut from_remote).await
    };
    let hello = match tokio::time::timeout(HANDSHAKE_TIMEOUT, handshake).await {
        Ok(Ok(frame)) => frame,
        Ok(Err(e)) => {
            let _ = child.wait().await;
            tokio::time::sleep(Duration::from_millis(50)).await;
            return Ok(failed(format!("{e:#}")));
        }
        Err(_) => return Ok(failed("the remote did not answer within 60s".to_string())),
    };
    match hello {
        Some(Frame::Hello { protocol, .. }) if protocol == upstream::PROTOCOL => {}
        // § adpzyyt56@r44hx
        Some(Frame::Hello { protocol, lade, .. }) => {
            return Ok(give_up(format!(
                "{} runs Lade {lade} (link protocol {protocol}) and this laptop Lade {} (link protocol {}). Upgrade one of them.",
                spec.host,
                env!("CARGO_PKG_VERSION"),
                upstream::PROTOCOL
            )));
        }
        Some(Frame::Failed { message, .. }) => return Ok(give_up(message)),
        _ => return Ok(failed("the remote did not answer as Lade".to_string())),
    }
    write_frame(
        &mut to_remote,
        &Frame::Hello {
            protocol: upstream::PROTOCOL,
            lade: env!("CARGO_PKG_VERSION").to_string(),
            host: super::hostname(),
        },
    )
    .await?;
    {
        let mut stdout = std::io::stdout();
        writeln!(stdout, "{LINK_READY}")?;
        stdout.flush()?;
    }

    let (out, writer) = Out::spawn(to_remote);
    let streams = Streams::default();
    let tunnels = Tunnels::default();
    let (in_tx, mut in_rx) = mpsc::unbounded_channel::<Option<Frame>>();
    tokio::spawn(async move {
        loop {
            let frame = read_frame(&mut from_remote).await.ok().flatten();
            let end = frame.is_none();
            if in_tx.send(frame).is_err() || end {
                break;
            }
        }
    });
    let spec = Arc::new(spec);
    let mut ping = tokio::time::interval(PING_EVERY);
    let mut last_seen = Instant::now();
    loop {
        tokio::select! {
            frame = in_rx.recv() => match frame {
                Some(Some(Frame::Resolve { id, walk, refs, recipient, report })) => {
                    last_seen = Instant::now();
                    let spec = Arc::clone(&spec);
                    let resolved = lookup(&walks, &walk);
                    let out = out.clone();
                    tokio::spawn(async move {
                        let reply = serve(&spec, resolved, id, &walk, refs, &recipient, &report).await;
                        out.send(reply);
                    });
                }
                Some(Some(Frame::TunnelOpen { id, walk, uri, check, report })) => {
                    last_seen = Instant::now();
                    let spec = Arc::clone(&spec);
                    let resolved = lookup(&walks, &walk);
                    let out = out.clone();
                    let tunnels = tunnels.clone();
                    tokio::spawn(async move {
                        let reply = open_tunnel(&spec, resolved, &tunnels, id, &walk, &uri, check, &report).await;
                        out.send(reply);
                    });
                }
                Some(Some(Frame::TunnelClose { id })) => {
                    last_seen = Instant::now();
                    tunnels.close(id);
                }
                Some(Some(Frame::StreamOpen { id, tunnel })) => {
                    last_seen = Instant::now();
                    tokio::spawn(open_stream(id, tunnels.port(tunnel), streams.clone(), out.clone()));
                }
                Some(Some(Frame::Detach)) => {
                    record(&spec, json!({ "event": "detach", "side": "remote" }), None, &Value::Null);
                    return Ok(LINK_DETACHED);
                }
                Some(Some(frame)) => {
                    last_seen = Instant::now();
                    streams.route(&frame);
                }
                Some(None) | None => {
                    let code = failed("the transport closed".to_string());
                    record(&spec, json!({ "event": "broken", "reason": "the transport closed" }), None, &Value::Null);
                    return Ok(if code == LINK_GIVE_UP { code } else { crate::exit_codes::FAILURE });
                }
            },
            _ = ping.tick() => {
                if last_seen.elapsed() > PONG_LOST {
                    record(&spec, json!({ "event": "broken", "reason": "no answer for 45s" }), None, &Value::Null);
                    return Ok(drop_link("the remote did not answer for 45s"));
                }
                out.send(Frame::Ping);
            }
            detach = hub_rx.recv() => {
                if detach == Some(true) {
                    out.send(Frame::Detach);
                    drop(out);
                    // The transport dies with this process: `attach` must
                    // read the detach and leave attached mode first.
                    let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
                    let _ = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
                    record(&spec, json!({ "event": "detach", "side": "laptop" }), None, &Value::Null);
                } else {
                    record(&spec, json!({ "event": "broken", "reason": "the laptop hub exited" }), None, &Value::Null);
                }
                return Ok(LINK_DETACHED);
            }
        }
    }
}

fn lookup(walks: &Walks, walk: &str) -> Option<Arc<ResolvedWalk>> {
    walks
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .get(walk)
        .cloned()
}

/// `w <hex> <json>` lines from the hub add walks; `d` detaches. The end of
/// the pipe without `d` means the hub is gone.
async fn lifeline(walks: Walks, hub: mpsc::UnboundedSender<bool>) {
    let mut lines = tokio::io::BufReader::new(tokio::io::stdin()).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        if line == LIFELINE_DETACH {
            let _ = hub.send(true);
            return;
        }
        let Some((walk, json)) = line
            .strip_prefix(LIFELINE_WALK)
            .and_then(|rest| rest.strip_prefix(' '))
            .and_then(|rest| rest.split_once(' '))
        else {
            continue;
        };
        let Ok(resolved) = serde_json::from_str::<ResolvedWalk>(json) else {
            continue;
        };
        walks
            .write()
            .unwrap_or_else(|e| e.into_inner())
            .insert(walk.to_string(), Arc::new(resolved));
        let mut stdout = std::io::stdout().lock();
        let _ = writeln!(stdout, "{LINK_WALK} {walk}");
        let _ = stdout.flush();
    }
    let _ = hub.send(false);
}

fn unknown_walk(spec: &LinkSpec) -> String {
    // § d0jxpxadk.a@0wa3s
    format!(
        "No lade connect resolved the lade.yaml files of this directory on {} as they are now",
        spec.host
    )
}

async fn serve(
    spec: &LinkSpec,
    resolved: Option<Arc<ResolvedWalk>>,
    id: u64,
    walk: &str,
    refs: Vec<String>,
    recipient: &str,
    report: &Value,
) -> Frame {
    // § d0jxpxadk@beg7x
    let started = Instant::now();
    let refuse = |code: u8, message: String| {
        record(
            spec,
            json!({ "event": "refused", "code": code, "reason": message, "walk": walk }),
            Some(&refs),
            report,
        );
        Frame::Failed { id, code, message }
    };
    let (Some(resolved), Some(walk_hash)) = (resolved, super::unhex(walk)) else {
        return refuse(upstream::WALK_UNKNOWN, unknown_walk(spec));
    };
    // § d0jxpxadk.b@pc1sj
    let mut templated = Vec::new();
    let mut outside = Vec::new();
    for r in &refs {
        if resolved.refs.contains_key(r) {
            continue;
        }
        match super::resolve::template_for(&resolved, r) {
            Some(template) => templated.push((r.clone(), template)),
            None => outside.push(r.clone()),
        }
    }
    if !outside.is_empty() {
        return refuse(
            upstream::REFUSED,
            format!(
                "The laptop's resolution of the lade.yaml files lade connect resolved yields no {}.",
                outside.join(", ")
            ),
        );
    }
    // § end d0jxpxadk.b
    let mut by_body = BTreeMap::<usize, BTreeSet<String>>::new();
    for r in &refs {
        if let Some((at, name)) = resolved.refs.get(r) {
            by_body.entry(*at).or_default().insert(name.clone());
        }
    }
    let failed = |e: anyhow::Error| {
        let message = format!("{e:#}");
        record(
            spec,
            json!({ "event": "failed", "reason": message, "walk": walk }),
            Some(&refs),
            report,
        );
        Frame::Failed {
            id,
            code: upstream::ABSENT,
            message,
        }
    };
    let mut values = HashMap::new();
    for (at, names) in &by_body {
        // § d0jxpxadk.c@5b9b8
        match super::resolve::values(&resolved, *at, walk_hash, names).await {
            Ok(found) => {
                for r in &refs {
                    if let Some((body, name)) = resolved.refs.get(r)
                        && body == at
                        && let Some(value) = found.get(name)
                    {
                        values.insert(r.clone(), value.clone());
                    }
                }
            }
            Err(e) => return failed(e),
        }
    }
    for (r, template) in templated {
        match super::resolve::templated_value(&resolved, template, &r, walk_hash).await {
            Ok(value) => {
                values.insert(r, value);
            }
            Err(e) => return failed(e),
        }
    }
    // § rp9r1x8pj@yv8nc
    let sealed = match serde_json::to_vec(&values)
        .map_err(anyhow::Error::from)
        .and_then(|plain| upstream::seal(recipient, &plain))
    {
        Ok(sealed) => sealed,
        Err(e) => {
            return Frame::Failed {
                id,
                code: crate::exit_codes::FAILURE as u8,
                message: format!("{e:#}"),
            };
        }
    };
    // § end rp9r1x8pj
    record_with(
        spec,
        json!({
            "event": "served",
            "walk": walk,
            "ms": started.elapsed().as_millis() as u64,
        }),
        Some(&refs),
        report,
        Some(values),
    );
    Frame::Resolved { id, sealed }
}

/// Laptop tunnels open for the remote, by the id `attach` gave each. Each
/// holds its own lease, so closing one leaves the others, and the hub drops
/// them all once this process is gone.
#[derive(Clone, Default)]
struct Tunnels(Arc<std::sync::Mutex<HashMap<u64, (u16, String)>>>);

impl Tunnels {
    fn port(&self, id: u64) -> Option<u16> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&id)
            .map(|(port, _)| *port)
    }

    fn close(&self, id: u64) {
        let lease = self
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id)
            .map(|(_, lease)| lease);
        if let Some(lease) = lease {
            tokio::task::spawn_blocking(move || crate::hub::tunnel_release(&lease));
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn open_tunnel(
    spec: &LinkSpec,
    resolved: Option<Arc<ResolvedWalk>>,
    tunnels: &Tunnels,
    id: u64,
    walk: &str,
    uri: &str,
    check: bool,
    report: &Value,
) -> Frame {
    let named = [uri.to_string()];
    let refuse = |code: u8, message: String| {
        record(
            spec,
            json!({ "event": "refused", "code": code, "reason": message, "walk": walk }),
            Some(&named),
            report,
        );
        Frame::Failed { id, code, message }
    };
    let Some(resolved) = resolved else {
        return refuse(upstream::WALK_UNKNOWN, unknown_walk(spec));
    };
    // § aswq5qe7f.d@30fbe
    let Some(own) = resolved.tunnels.get(uri).cloned() else {
        return refuse(
            upstream::REFUSED,
            format!(
                "The laptop's resolution of the lade.yaml files lade connect resolved yields no tunnel {uri}."
            ),
        );
    };
    if check {
        record(
            spec,
            json!({ "event": "checked", "walk": walk }),
            Some(&named),
            report,
        );
        return Frame::TunnelReady { id };
    }
    let started = Instant::now();
    let host = spec.host.clone();
    let lease_id = format!("r{}-{id}", std::process::id());
    let opened = {
        let lease_id = lease_id.clone();
        tokio::task::spawn_blocking(move || {
            // § aswq5qe7f.a@ywekb
            let lease = crate::network::lease_for(lease_id, std::process::id())?;
            crate::network::open_for_remote(&host, &own, lease)
        })
        .await
        .map_err(anyhow::Error::from)
        .and_then(|opened| opened)
    };
    match opened {
        Ok(port) => {
            tunnels
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(id, (port, lease_id));
            record(
                spec,
                json!({ "event": "tunnel", "walk": walk, "ms": started.elapsed().as_millis() as u64 }),
                Some(&named),
                report,
            );
            Frame::TunnelReady { id }
        }
        Err(e) => {
            tokio::task::spawn_blocking(move || crate::hub::tunnel_release(&lease_id));
            let message = format!("{e:#}");
            record(
                spec,
                json!({ "event": "failed", "reason": message }),
                Some(&named),
                report,
            );
            Frame::Failed {
                id,
                code: crate::exit_codes::FAILURE as u8,
                message,
            }
        }
    }
}

/// One remote connection to the laptop tunnel `port` serves.
async fn open_stream(id: u64, port: Option<u16>, streams: Streams, out: Out) {
    let failed = |message: String| Frame::Failed {
        id,
        code: crate::exit_codes::FAILURE as u8,
        message,
    };
    let Some(port) = port else {
        out.send(failed("the laptop tunnel is closed".to_string()));
        return;
    };
    match tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
        Ok(socket) => {
            let opened = streams.open(id);
            out.send(Frame::StreamReady { id });
            streams.pump(socket, id, opened, out).await;
        }
        Err(e) => {
            out.send(failed(format!("cannot reach the laptop tunnel: {e}")));
        }
    }
}

/// Every link event, whatever the remote rules' `log` says.
pub(crate) fn record(spec: &LinkSpec, detail: Value, refs: Option<&[String]>, report: &Value) {
    record_with(spec, detail, refs, report, None);
}

/// `served`: the values just handed out, scrubbed from the reported command.
fn record_with(
    spec: &LinkSpec,
    detail: Value,
    refs: Option<&[String]>,
    report: &Value,
    served: Option<HashMap<String, String>>,
) {
    // § b4tp4hvxk@cz915
    // The host as the laptop dialed it. The name the remote gave itself is
    // its own report.
    // § b4tp4hvxk.a@h9bv1
    let dialed = Transport::parse(&spec.transport)
        .map_or_else(|_| spec.transport.join(" "), |t| t.host().to_string());
    let mut agent = json!({
        "remote": detail,
        "host": dialed,
        "reported_host": spec.host,
        "transport": spec.written,
    });
    // § end b4tp4hvxk.a
    // The argv goes through the diary's scrubbing below, not here verbatim.
    if let Some(reported) = report.as_object() {
        let kept = reported
            .iter()
            .filter(|(key, _)| key.as_str() != "argv")
            .map(|(key, value)| {
                let value = match value {
                    Value::String(text) => Value::String(crate::scrub::cap_text(text.clone()).0),
                    _ => value.clone(),
                };
                (key.clone(), value)
            })
            .collect();
        agent["reported"] = Value::Object(kept);
    }
    let audience = match report.get("audience").and_then(Value::as_str) {
        Some("agent") => Audience::Agent,
        _ => Audience::Human,
    };
    let argv = report.get("argv").cloned();
    let command = argv
        .as_ref()
        .and_then(Value::as_array)
        .map(|argv| {
            argv.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|| format!("lade connect -- {}", spec.written));
    let cwd = report
        .get("cwd")
        .and_then(Value::as_str)
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let bindings = refs
        .unwrap_or_default()
        .iter()
        .map(|uri| json!({ "uri": uri }))
        .collect::<Vec<_>>();
    // § kk9qxrvgt.c@qx86q
    // silcrow:follow
    event::emit_unstamped(Emit {
        kind: EventKind::Remote,
        via: Via::Unknown,
        audience,
        actor: None,
        cwd,
        command,
        argv,
        // § b4tp4hvxk.c@amsdv
        hydrated: served,
        // § end b4tp4hvxk.c
        matches: json!([{ "bindings": bindings }]),
        hydrate_ms: None,
        agent: Some(agent),
    });
    // § end kk9qxrvgt.c
}
