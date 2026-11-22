use std::path::Path;

use chrono::Local;

use crate::command::ttl::{format_duration_ms, format_ttl_left, window_with_cap};
use crate::hub::HubState;
use crate::preexec::path_for_display;

use super::format::{Printer, format_bytes, format_tried_at};
use super::{GlobalConfigInfo, HubStatus, StatusReport};

pub(super) fn print_human(report: &StatusReport) {
    let out = Printer::new();
    print_lade(&out, report);
    print_preexec(&out, report);
    print_pretool(&out, report);
    print_hub(&out, &report.hub, &report.global_config);
    print_mise(&out, report);
    print_remote(&out, &report.remote);
    print_project(&out, report);
}

fn print_remote(out: &Printer, remote: &super::RemoteStatus) {
    if remote.upstream == "none" && remote.links.is_empty() {
        return;
    }
    out.blank();
    out.heading("Remote");
    let mut rows = Vec::new();
    if remote.upstream != "none" {
        rows.push((
            "upstream",
            format!(
                "{} to {}",
                remote.upstream,
                remote.laptop.as_deref().unwrap_or("an unknown laptop")
            ),
        ));
    }
    for link in &remote.links {
        let error = link
            .error
            .as_deref()
            .map(|e| format!(" ({e})"))
            .unwrap_or_default();
        rows.push(("link", format!("{} {}{error}", link.host, link.phase)));
    }
    out.pairs(&rows);
}

fn print_lade(out: &Printer, report: &StatusReport) {
    let v = &report.version;
    out.heading(&format!("Lade {}", v.current));
    // § rbzjhj8tk.d@hc37k
    let latest = match (&v.latest, v.update_available) {
        (Some(latest), true) => format!("{latest} (update available. run `lade upgrade`)"),
        (Some(latest), false) => format!("{latest} (up to date)"),
        (None, _) => match (v.last_check, report.hub.info.state) {
            (Some(at), _) => format!("({})", format_tried_at(at, Local::now())),
            (None, HubState::Up) => "(not checked yet)".to_string(),
            (None, _) => "(unknown, the hub is not running)".to_string(),
        },
    };
    let mut rows = vec![("latest", latest)];
    if let (Some(err), Some(at)) = (&v.check_error, v.last_check) {
        rows.push((
            "last check",
            format!("failed ({}): {err}", format_tried_at(at, Local::now())),
        ));
    }
    // § end rbzjhj8tk.d
    rows.push((
        "age plugin",
        match &report.age_plugin.path {
            Some(path) => path_for_display(path),
            None => "missing (run `lade upgrade` or `cargo install age-plugin-lade`)".to_string(),
        },
    ));
    rows.push((
        "global config",
        path_for_display(&report.global_config.path),
    ));
    rows.push((
        "user",
        report
            .global_config
            .user
            .clone()
            .unwrap_or_else(|| "(OS default)".to_string()),
    ));
    rows.push((
        "log",
        format!(
            "{} ({} events, {})",
            path_for_display(&report.log.path),
            report.log.events,
            format_bytes(report.log.bytes)
        ),
    ));
    out.pairs(&rows);
}

fn print_preexec(out: &Printer, report: &StatusReport) {
    out.blank();
    let Some(preexec) = &report.hooks.preexec else {
        out.heading("Pre-exec");
        out.pairs(&[(
            "shell",
            "none started Lade: run `lade status` from bash, zsh, or fish".to_string(),
        )]);
        return;
    };
    out.heading(&format!("Pre-exec ({})", preexec.shell));
    out.pairs(&[
        ("profile", path_for_display(&preexec.profile)),
        (
            "installed",
            if preexec.installed {
                "yes".to_string()
            } else {
                "no (run `lade hook enable --shell`)".to_string()
            },
        ),
        (
            "inject wrap",
            match &preexec.inject_startup_skipped {
                Some(name) => format!("skips startup files ({name} present)"),
                None => "skips startup files (bashrc, zshenv, fish config)".to_string(),
            },
        ),
    ]);
}

fn print_pretool(out: &Printer, report: &StatusReport) {
    let pretool = &report.hooks.pretool;
    out.blank();
    out.heading("Pre-tool");
    let mut rows = Vec::new();
    for (name, status) in [
        ("Cursor", &pretool.cursor),
        ("Claude Code", &pretool.claude),
        ("Codex", &pretool.codex),
        ("OpenCode", &pretool.opencode),
    ] {
        for (scope, location) in [("machine", &status.global), ("repo", &status.project)] {
            rows.push(vec![
                name.to_string(),
                scope.to_string(),
                pretool_flag(
                    location.installed,
                    location.current,
                    location.error.is_some(),
                )
                .to_string(),
                path_for_display(&location.path),
            ]);
        }
        if let Some(committed) = &status.committed {
            rows.push(vec![
                name.to_string(),
                "committed".to_string(),
                committed_flag(
                    committed.present,
                    committed.blocking,
                    committed.error.is_some(),
                )
                .to_string(),
                path_for_display(&committed.path),
            ]);
        }
    }
    out.table(&["HARNESS", "SCOPE", "STATE", "PATH"], &rows);
    for notice in &report.hook_notices {
        out.line(&format!("! {notice}"));
    }
}

fn print_hub(out: &Printer, hub: &HubStatus, config: &GlobalConfigInfo) {
    out.blank();
    out.heading("Hub");
    let info = &hub.info;
    // § rbzjhj8tk.a@rw89c
    let state = match (info.state, info.pid) {
        (HubState::Down, _) => "down".to_string(),
        (HubState::Stale, _) => "stale".to_string(),
        (HubState::Up, Some(pid)) => format!("pid {pid}"),
        (HubState::Up, None) => "up".to_string(),
    };
    // § end rbzjhj8tk.a
    // § cfan04ksd.e@b5r6m
    let mut max_cache = match (&config.max_cache_error, config.max_cache_ms) {
        (Some(err), _) => format!("invalid, cache off: {err}"),
        (None, None) => "none (`max_cache` in config caps every ttl)".to_string(),
        (None, Some(0)) => "0, cache off".to_string(),
        (None, Some(ms)) => format_duration_ms(ms),
    };
    let config_cap = match config.max_cache_error {
        Some(_) => Some(0),
        None => config.max_cache_ms,
    };
    if info.state == HubState::Up && hub.max_cache_ms != config_cap {
        max_cache.push_str(&format!(
            " (the hub still enforces {}, it rereads the config every second)",
            match hub.max_cache_ms {
                None => "none".to_string(),
                Some(0) => "0".to_string(),
                Some(ms) => format_duration_ms(ms),
            }
        ));
    }
    // § end cfan04ksd.e
    let mut rows = vec![
        ("state", state),
        ("max cache", max_cache),
        ("guard", memory_line(&hub.memory)),
    ];
    if info.state == HubState::Up {
        rows.push((
            "cache",
            format!(
                "{} keys ({} for this cwd), {} tunnels, {} tickets",
                hub.key_count,
                hub.keys.len(),
                hub.tunnels.len(),
                info.tickets
            ),
        ));
        rows.push((
            "window",
            match &hub.window {
                Some(ttl) => format!("{} for this cwd", window_with_cap(ttl, hub.max_cache_ms)),
                None => "none for this cwd (`lade ttl set` marks one)".to_string(),
            },
        ));
    }
    out.pairs(&rows);
    if !hub.keys.is_empty() {
        out.table(
            &["KEY", "CWD", "TTL"],
            &hub.keys
                .iter()
                .map(|key| {
                    vec![
                        key.name.clone(),
                        path_for_display(Path::new(&key.cwd)),
                        format_ttl_left(key.ttl_left_ms),
                    ]
                })
                .collect::<Vec<_>>(),
        );
    }
    if !hub.tunnels.is_empty() {
        out.table(
            &["PORT", "KEY", "URI", "STATE"],
            &hub.tunnels
                .iter()
                .map(|tunnel| {
                    vec![
                        tunnel.port.to_string(),
                        tunnel.key.clone(),
                        tunnel.uri.clone(),
                        tunnel.state.clone(),
                    ]
                })
                .collect::<Vec<_>>(),
        );
    }
}

fn print_mise(out: &Printer, report: &StatusReport) {
    let mise = &report.mise;
    out.blank();
    let heading = match (mise.needed, mise.in_range, &mise.version) {
        (false, _, _) => "mise (not needed)".to_string(),
        (true, true, Some(version)) => format!("mise {version} (in range {})", mise.range),
        (true, true, None) => format!("mise (in range {})", mise.range),
        (true, false, Some(version)) => {
            format!("mise {version} (outside {}, run `lade setup`)", mise.range)
        }
        (true, false, None) => "mise missing (run `lade setup`)".to_string(),
    };
    out.heading(&heading);
    if mise.tools.is_empty() {
        return;
    }
    out.table(
        &["TOOL", "VERSION", "STORE"],
        &mise
            .tools
            .iter()
            .map(|tool| {
                vec![
                    tool.name.clone(),
                    tool.version.clone().unwrap_or_else(|| "-".to_string()),
                    if tool.present {
                        "yes".to_string()
                    } else {
                        "missing (run `lade setup`)".to_string()
                    },
                ]
            })
            .collect::<Vec<_>>(),
    );
}

fn print_project(out: &Printer, report: &StatusReport) {
    let pc = &report.project_config;
    out.blank();
    out.heading("Project");
    if let Some(err) = &pc.error {
        out.pairs(&[("config", "error".to_string())]);
        out.line(err);
        return;
    }
    out.pairs(&[("config", format!("ok ({} rules)", pc.rule_count))]);
    out.blank();
    out.heading("Providers");
    if pc.providers.is_empty() {
        out.line("none referenced in lade.yaml (`lade status --all` lists every one)");
        return;
    }
    out.table(
        &["SCHEME", "PROVIDER", "VIA", "TIMEOUT", "BATCH BY"],
        &pc.providers
            .iter()
            .map(|provider| {
                let via = match provider.transport.as_str() {
                    "sdk" => format!(
                        "sdk {}",
                        provider.version.as_deref().unwrap_or(lade_sdk::VERSION)
                    ),
                    other => other.to_string(),
                };
                vec![
                    provider.scheme.clone(),
                    provider.name.clone(),
                    via,
                    format!("{}s", provider.timeout_s),
                    provider.batch_unit.clone(),
                ]
            })
            .collect::<Vec<_>>(),
    );
}

fn memory_line(guard: &crate::hub::MemoryGuard) -> String {
    use crate::hub::MemoryGuard;
    // § rbzjhj8tk.b@vyav1
    match guard {
        MemoryGuard::HardenedRuntime => "cache on: hardened runtime, no debugger attaches".into(),
        MemoryGuard::NotDumpable => "cache on: hub not dumpable, no debugger attaches".into(),
        MemoryGuard::DebugBuild => "cache on: debug build, no memory guard".into(),
        MemoryGuard::Off { detail } => format!("cache off: {detail}"),
    }
}

pub(super) fn committed_flag(present: bool, blocking: bool, unreadable: bool) -> &'static str {
    // § khcwz2ec3.d@0vaqx
    match (unreadable, present, blocking) {
        (true, _, _) => "unreadable",
        (false, true, true) => "blocking",
        (false, true, false) => "not blocking",
        (false, false, _) => "none",
    }
}

pub(super) fn pretool_flag(installed: bool, current: bool, unreadable: bool) -> &'static str {
    // § khcwz2ec3.d@h2vt0
    match (unreadable, installed, current) {
        (true, _, _) => "unreadable",
        (false, true, true) => "current",
        (false, true, false) => "not current",
        (false, false, _) => "missing",
    }
}
