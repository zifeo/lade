//! `lade connect` and `lade detach` on the laptop.

use anyhow::{Context, Result, bail};
use serde_json::json;
use std::io::IsTerminal;
use std::process::Stdio;
use std::time::{Duration, Instant};

use super::files::{self, RemoteFiles};
use super::link::LinkSpec;
use super::transport::{Kind, Transport, quote};
use crate::config::Audience;
use crate::context::InvocationContext;
use crate::hub::{LinkPhase, LinkRow};
use crate::message_box::Report;

const READY_TIMEOUT: Duration = Duration::from_secs(90);
/// The master `lade connect` opens for `__remote` to ride: short, so another
/// process of the laptop cannot reuse a session for long.
const CONTROL_PERSIST: &str = "ControlPersist=60";

fn key(transport: &Transport) -> String {
    transport.argv.join("\u{0}")
}

/// `<cache root>/t/cm-<12 hex of the transport>`: ssh's own `%C` is 40 hex
/// long, which breaks macOS's 104-byte socket path cap under a deep cache.
fn control_path(transport: &Transport) -> Result<String> {
    use sha2::Digest;
    let dir = crate::cache::tunnel_sockets();
    std::fs::create_dir_all(&dir)?;
    let digest = sha2::Sha256::digest(key(transport).as_bytes());
    let path = dir
        .join(format!("cm-{}", &super::hex(&digest)[..12]))
        .to_string_lossy()
        .into_owned();
    // ssh binds `<path>.<16 random chars>` first, then renames it.
    if path.len() + 17 >= 104 {
        bail!(
            "The ssh control socket {path} is too long for a unix socket once ssh adds its 17-byte suffix. Set LADE_CACHE_DIR to a shorter path."
        );
    }
    Ok(path)
}

fn fetch(transport: &Transport, path: &str, control: Option<&str>) -> Result<RemoteFiles> {
    let remote = format!("lade __remote-files {}", quote(path));
    let mut options = Vec::new();
    if let Some(path) = control {
        options.push("ControlMaster=auto".to_string());
        options.push(format!("ControlPath={path}"));
        options.push(CONTROL_PERSIST.to_string());
    }
    if !std::io::stdin().is_terminal() && transport.kind == Kind::Ssh {
        options.push("BatchMode=yes".to_string());
    }
    let output = transport
        .command(&options, &remote)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .output()
        .with_context(|| format!("cannot run {}", transport.argv[0]))?;
    if !output.status.success() {
        bail!(
            "{} exited with {} before Lade answered on {}: is lade on its non-interactive PATH?",
            transport.argv[0],
            output.status,
            transport.host()
        );
    }
    let answer = files::parse(&output.stdout)?;
    if answer.protocol != lade_sdk::upstream::PROTOCOL {
        bail!(
            "{} runs Lade {} (link protocol {}) and this laptop Lade {} (link protocol {}). Upgrade one of them.",
            answer.host,
            answer.lade,
            answer.protocol,
            env!("CARGO_PKG_VERSION"),
            lade_sdk::upstream::PROTOCOL
        );
    }
    Ok(answer)
}

fn view(answer: &RemoteFiles, walk: &str) {
    // § ycvygtrew.a@5xm20
    use super::shown;
    let mut report = Report::new()
        .heading(format!(
            "{}  lade {}  protocol {}",
            shown(&answer.host),
            shown(&answer.lade),
            answer.protocol
        ))
        .line(format!(
            "walk {}  from {}",
            super::short_walk(walk),
            shown(&answer.dir)
        ));
    for file in &answer.files {
        report = report.dim(format!("── {} ──", shown(&file.path)));
        for line in file.text.lines() {
            report = report.line(format!("  {}", shown(line)));
        }
    }
    report.print();
}

fn wait_ready(key: &str, host: &str, walk: &str) -> Result<LinkRow> {
    let deadline = Instant::now() + READY_TIMEOUT;
    loop {
        // A hub busy past one poll's deadline answers the next one.
        let Some(rows) = crate::hub::link_poll(key) else {
            if Instant::now() > deadline {
                bail!("the hub did not answer for {}s", READY_TIMEOUT.as_secs());
            }
            std::thread::sleep(Duration::from_millis(100));
            continue;
        };
        match rows.into_iter().next() {
            Some(row) if row.phase == LinkPhase::Ready && row.walks.iter().any(|w| w == walk) => {
                return Ok(row);
            }
            Some(LinkRow {
                phase: LinkPhase::Failed | LinkPhase::Stopped,
                error,
                ..
            }) => bail!(
                "the link to {host} did not come up: {}",
                error.as_deref().unwrap_or("no reason given")
            ),
            Some(_) => {}
            None => bail!("the hub lost the link to {host}"),
        }
        if Instant::now() > deadline {
            bail!(
                "the link to {host} was not ready within {}s",
                READY_TIMEOUT.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub(crate) async fn run(
    ctx: &InvocationContext,
    path: String,
    transport: Vec<String>,
) -> Result<()> {
    // § ycvygtrew@pxb9m
    let transport = Transport::parse(&transport)?;
    let control = match transport.kind {
        Kind::Ssh => Some(control_path(&transport)?),
        Kind::Tsh => None,
    };
    let answer = fetch(&transport, &path, control.as_deref())?;
    let walk = answer.walk();
    let walk_hex = super::hex(&walk);
    view(&answer, &walk_hex);
    let spec = LinkSpec {
        transport: transport.argv.clone(),
        written: transport.written(),
        host: answer.host.clone(),
        laptop: super::machine_name(),
        control_path: control,
    };
    let env = std::env::vars_os()
        .map(|(k, v)| {
            use std::os::unix::ffi::OsStrExt;
            (k.as_bytes().to_vec(), v.as_bytes().to_vec())
        })
        .collect::<Vec<_>>();
    let key = key(&transport);
    // Now, while the master of the fetch lives: a resolution with prompts
    // can outlast its ControlPersist, and a fresh dial without a terminal
    // fails on a host that asks for a second factor.
    if crate::hub::link_start(
        key.clone(),
        answer.host.clone(),
        serde_json::to_string(&spec)?,
        env.clone(),
        None,
    )
    .is_none()
    {
        bail!("the hub did not start: lade connect needs it to hold the link");
    }
    let by = match ctx.audience {
        Audience::Agent => "agent",
        Audience::Human => "human",
    };
    let (resolved, failed) = super::resolve::resolve_walk(
        &answer.path,
        &answer.dir,
        answer.files.clone(),
        answer.user.clone(),
        by,
        walk,
    )
    .await?;
    // § ycvygtrew.b@jyf5t
    if !failed.is_empty() {
        bail!(
            "This laptop could not resolve the walk of {}: {}",
            super::shown(&answer.dir),
            failed.join("; ")
        );
    }
    let mut notes = Report::new();
    for why in &resolved.deferred {
        notes = notes.dim(format!("on first use: {}", super::shown(why)));
    }
    notes.print();
    // § ycvygtrew.c@bzq8h
    let started = crate::hub::link_start(
        key.clone(),
        answer.host.clone(),
        serde_json::to_string(&spec)?,
        env,
        Some((walk_hex.clone(), serde_json::to_string(&resolved)?)),
    );
    if started.is_none() {
        bail!("the hub did not start: lade connect needs it to hold the link");
    }
    let row = wait_ready(&key, &answer.host, &walk_hex)?;
    // § end ycvygtrew.c
    // § end ycvygtrew.b
    // § b4tp4hvxk.b@3hpgk
    super::link::record(
        &spec,
        json!({
            "event": "connect",
            "by": by,
            "path": answer.path,
            "walk": walk_hex,
            "files": resolved.files.iter().map(|file| &file.path).collect::<Vec<_>>(),
            "refs": resolved.refs.keys().collect::<Vec<_>>(),
            "tunnels": resolved.tunnels,
            "deferred": resolved.deferred,
        }),
        Some(&resolved.refs.keys().cloned().collect::<Vec<_>>()),
        &json!({ "cwd": answer.dir, "audience": by }),
    );
    // § end b4tp4hvxk.b
    Report::new()
        .line(format!(
            "Linked {} for {}: {} refs and {} tunnels resolve through this laptop.",
            row.host,
            super::shown(&answer.dir),
            resolved.refs.len(),
            resolved.tunnels.len()
        ))
        .dim(format!("Stop: lade detach -- {}", transport.written()))
        .print();
    Ok(())
}

pub(crate) fn detach(transport: Vec<String>) -> Result<()> {
    if transport.is_empty() {
        return super::attach::detach_here();
    }
    let transport = Transport::parse(&transport)?;
    let key = key(&transport);
    let ready = crate::hub::link_poll(&key)
        .is_some_and(|rows| rows.iter().any(|row| row.phase == LinkPhase::Ready));
    let stopped = crate::hub::link_stop(&key) == Some(true);
    if ready {
        Report::new()
            .line(format!(
                "Detached {}. Values it already holds expire with their ttl.",
                transport.host()
            ))
            .print();
        return Ok(());
    }
    // § q4qz9zrdn.b@c26pd
    // No Detach frame reaches a remote whose link is down: its marker would
    // keep it failing with exit 4.
    let mut options = Vec::new();
    if !std::io::stdin().is_terminal() && transport.kind == Kind::Ssh {
        options.push("BatchMode=yes".to_string());
    }
    let output = transport
        .command(&options, "lade detach")
        .stdin(Stdio::inherit())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("cannot run {}", transport.argv[0]))?;
    if !output.status.success() {
        bail!(
            "{}{} could not run lade detach on {} ({}): run lade detach there.",
            if stopped {
                "Stopped the link, but "
            } else {
                ""
            },
            transport.argv[0],
            transport.host(),
            super::shown(String::from_utf8_lossy(&output.stderr).trim())
        );
    }
    Report::new()
        .line(format!(
            "Detached {}: lade detach ran there. Values it already holds expire with their ttl.",
            transport.host()
        ))
        .print();
    Ok(())
}
