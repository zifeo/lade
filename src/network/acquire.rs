use anyhow::Result;
use std::collections::HashMap;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::config::NetworkBinding;
use crate::network::command::{build_command, ensure_provider_preflight};
use crate::network::parse::{parse_binding, reconcile_local_port};
use crate::network::process::{
    ChildOutputFiles, RunningForward, configure_child_process, stop_network_pids_list,
    wait_child_ready,
};
use crate::network::progress::{ProviderProgressEvent, ProviderProgressKind, format_timing};
use crate::network::types::{
    AcquiredNetwork, DetachedNetworkSession, LocalTarget, ParsedBinding, ProviderSpec,
};
use crate::provider_progress::ProviderProgressSink;

const DEFAULT_READY_TIMEOUT: Duration = Duration::from_secs(20);

pub fn start_attached_network_session(
    bindings: &[NetworkBinding],
    progress: ProviderProgressSink,
) -> Result<AcquiredNetwork> {
    if bindings.is_empty() {
        return Ok(AcquiredNetwork::empty());
    }
    let mut env = HashMap::new();
    let mut sources = Vec::new();
    let mut guards = Vec::new();
    let handles = bindings
        .iter()
        .cloned()
        .map(|binding| {
            let progress = progress.clone();
            std::thread::spawn(move || acquire_attached_binding(binding, progress))
        })
        .collect::<Vec<_>>();
    for handle in handles {
        let attached = handle
            .join()
            .map_err(|_| anyhow::anyhow!("network provider worker panicked"))
            .and_then(|inner| inner)?;
        if let Some((key, value)) = attached.env_entry {
            env.insert(key, value);
        }
        sources.push(attached.source_uri);
        guards.push(attached.guard);
    }
    Ok(AcquiredNetwork {
        env,
        sources,
        _guards: guards,
    })
}

pub fn start_detached_network_session(
    bindings: &[NetworkBinding],
    progress: ProviderProgressSink,
) -> Result<DetachedNetworkSession> {
    if bindings.is_empty() {
        return Ok(DetachedNetworkSession::empty());
    }
    let mut env = HashMap::new();
    let mut pids = Vec::new();
    let handles = bindings
        .iter()
        .cloned()
        .map(|binding| {
            let progress = progress.clone();
            std::thread::spawn(move || acquire_detached_binding(binding, progress))
        })
        .collect::<Vec<_>>();
    for handle in handles {
        let outcome = handle
            .join()
            .map_err(|_| anyhow::anyhow!("network provider worker panicked"))
            .and_then(|inner| inner);
        let (env_entry, pid) = match outcome {
            Ok(value) => value,
            Err(e) => {
                if !pids.is_empty() {
                    stop_network_pids_list(&pids);
                }
                return Err(e);
            }
        };
        if let Some((key, value)) = env_entry {
            env.insert(key, value);
        }
        pids.push(pid);
    }
    Ok(DetachedNetworkSession { env, pids })
}

struct PreparedBinding {
    parsed: ParsedBinding,
    local_host: String,
    local_port: u16,
    cmd: Command,
    progress_id: String,
    display: String,
    started: Instant,
}

fn prepare_binding(
    binding: NetworkBinding,
    progress: &ProviderProgressSink,
) -> Result<PreparedBinding> {
    let started = Instant::now();
    let parsed = parse_binding(&binding)?;
    let local_port = reconcile_local_port(&parsed.target, parsed.local_port, &parsed.local_host)?;
    let local_host = parsed.local_host.clone();
    let progress_id = format!("{}|{}", binding.key, binding.uri);
    let display = connection_label(&parsed.spec, &local_host, local_port);
    send_progress(
        progress,
        &progress_id,
        display.clone(),
        ProviderProgressKind::Connecting,
    );
    if let Err(e) = ensure_provider_preflight(&parsed.spec) {
        send_failed(progress, progress_id, display, started);
        return Err(e);
    }
    let cmd = match build_command(&parsed.spec, &local_host, local_port) {
        Ok(cmd) => cmd,
        Err(e) => {
            send_failed(progress, progress_id, display, started);
            return Err(e);
        }
    };
    Ok(PreparedBinding {
        parsed,
        local_host,
        local_port,
        cmd,
        progress_id,
        display,
        started,
    })
}

struct AttachedBinding {
    env_entry: Option<(String, String)>,
    source_uri: String,
    guard: RunningForward,
}

fn acquire_attached_binding(
    binding: NetworkBinding,
    progress: ProviderProgressSink,
) -> Result<AttachedBinding> {
    let PreparedBinding {
        mut parsed,
        local_host,
        local_port,
        cmd,
        progress_id,
        display,
        started,
    } = prepare_binding(binding, &progress)?;
    let label = provider_label(&parsed.spec);
    let mut process = match RunningForward::spawn(label, cmd) {
        Ok(process) => process,
        Err(e) => {
            send_failed(&progress, progress_id, display, started);
            return Err(e);
        }
    };
    if let Err(e) = process.wait_ready(&local_host, local_port, DEFAULT_READY_TIMEOUT) {
        send_failed(&progress, progress_id, display, started);
        return Err(e);
    }
    let pid = process.child.id();
    let env_entry = env_entry_for(&parsed.target, local_port);
    let connected = format!(
        "{} pid={} {} ms",
        connection_label(&parsed.spec, &local_host, local_port),
        pid,
        started.elapsed().as_millis()
    );
    send_progress(
        &progress,
        &progress_id,
        connected,
        ProviderProgressKind::Connected,
    );
    Ok(AttachedBinding {
        env_entry,
        source_uri: std::mem::take(&mut parsed.source_uri),
        guard: process,
    })
}

fn acquire_detached_binding(
    binding: NetworkBinding,
    progress: ProviderProgressSink,
) -> Result<(Option<(String, String)>, u32)> {
    let PreparedBinding {
        parsed,
        local_host,
        local_port,
        mut cmd,
        progress_id,
        display,
        started,
    } = prepare_binding(binding, &progress)?;
    configure_child_process(&mut cmd);
    let logs = ChildOutputFiles::capture(&mut cmd)?;
    let mut child = match cmd.spawn() {
        Ok(child) => child,
        Err(e) => {
            logs.cleanup();
            send_failed(&progress, progress_id, display, started);
            return Err(e.into());
        }
    };
    if let Err(e) = wait_child_ready(&mut child, &local_host, local_port, DEFAULT_READY_TIMEOUT) {
        let _ = child.kill();
        let _ = child.wait();
        let log_text = logs.read_text();
        logs.cleanup();
        send_failed(&progress, progress_id, display, started);
        if log_text.is_empty() {
            return Err(e);
        }
        return Err(anyhow::anyhow!("{e}\n{log_text}"));
    }
    logs.cleanup();
    let pid = child.id();
    let env_entry = env_entry_for(&parsed.target, local_port);
    let connected = format!(
        "{} pid={} {} ms",
        connection_label(&parsed.spec, &local_host, local_port),
        pid,
        started.elapsed().as_millis()
    );
    send_progress(
        &progress,
        &progress_id,
        connected,
        ProviderProgressKind::Connected,
    );
    Ok((env_entry, pid))
}

fn env_entry_for(target: &LocalTarget, local_port: u16) -> Option<(String, String)> {
    match target {
        LocalTarget::EnvVar(name) => Some((name.clone(), local_port.to_string())),
        LocalTarget::FixedPort(_) => None,
    }
}

fn connection_label(spec: &ProviderSpec, local_host: &str, local_port: u16) -> String {
    let local = if local_host == "127.0.0.1" || local_host == "localhost" {
        local_port.to_string()
    } else {
        format!("{local_host}:{local_port}")
    };
    match spec {
        ProviderSpec::Kubectl {
            name, remote_port, ..
        } => format!("{name}:{remote_port} on {local}"),
        ProviderSpec::Kubefwd {
            name, service_port, ..
        } => format!("{name}:{service_port} on {local}"),
        ProviderSpec::Tsh {
            name, remote_port, ..
        } => format!("{name}:{remote_port} on {local}"),
        ProviderSpec::Ssh {
            remote_host,
            remote_port,
            ..
        } => format!("{remote_host}:{remote_port} on {local}"),
    }
}

fn provider_label(spec: &ProviderSpec) -> &'static str {
    match spec {
        ProviderSpec::Kubectl { .. } => "kubectl forward",
        ProviderSpec::Kubefwd { .. } => "kubefwd forward",
        ProviderSpec::Tsh { .. } => "tsh forward",
        ProviderSpec::Ssh { .. } => "ssh forward",
    }
}

fn send_failed(progress: &ProviderProgressSink, id: String, display: String, started: Instant) {
    send_progress(
        progress,
        &id,
        format_timing(&display, started),
        ProviderProgressKind::Failed,
    );
}

fn send_progress(
    progress: &ProviderProgressSink,
    id: &str,
    display: String,
    kind: ProviderProgressKind,
) {
    progress.send(ProviderProgressEvent {
        id: id.to_string(),
        display,
        kind,
    });
}
