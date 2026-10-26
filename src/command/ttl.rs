use anyhow::Result;
use serde_json::{Value, json};
use std::path::Path;

use crate::args::{TtlAction, TtlCommand};
use crate::config::RuleTtl;
use crate::hub::{self, CacheListing, CacheRow, HubState, TunnelPhase, TunnelRow};
use crate::message_box::Report;

pub fn run(opts: TtlCommand) -> Result<()> {
    match opts.action {
        None | Some(TtlAction::List) => print_list(opts.json, opts.global),
        Some(TtlAction::Forget { names }) if names.is_empty() => forget_all(),
        Some(TtlAction::Forget { names }) => forget_named(&names),
        Some(TtlAction::Set { ttl }) => set_window(ttl.as_deref()),
        Some(TtlAction::Unset) => clear_window(),
    }
}

fn set_window(ttl: Option<&str>) -> Result<()> {
    let Some(raw) = ttl else {
        anyhow::bail!("the ttl is required");
    };
    let raw = raw.trim();
    RuleTtl::parse(raw).map_err(|e| anyhow::anyhow!("{e}"))?;
    let scope = current_scope()?;
    match hub::set_window(&scope, raw) {
        Some(()) => {
            Report::new()
                .line(format!(
                    "ttl {raw}  {}",
                    crate::preexec::path_for_display(Path::new(&scope))
                ))
                .print();
            Ok(())
        }
        None => hub_unavailable(),
    }
}

fn clear_window() -> Result<()> {
    let scope = current_scope()?;
    match hub::unset_window(&scope) {
        Some(()) => {
            Report::new()
                .line(format!(
                    "ttl cleared  {}",
                    crate::preexec::path_for_display(Path::new(&scope))
                ))
                .print();
            Ok(())
        }
        None => hub_unavailable(),
    }
}

fn current_scope() -> Result<String> {
    let cwd = std::env::current_dir()?;
    Ok(hub::scope(&cwd))
}

fn hub_unavailable() -> Result<()> {
    anyhow::bail!(match hub::probe().state {
        HubState::Stale => "hub: stale",
        _ => "hub: down",
    })
}

fn print_list(as_json: bool, global: bool) -> Result<()> {
    let listing = hub::listing();
    let scope = current_scope().ok();
    let window = scope.as_deref().and_then(hub::window);
    let rows = rows_for_scope(&listing.rows, scope.as_deref(), global);
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&listing_json(
                &listing,
                window.as_deref(),
                &rows,
                global
            ))?
        );
        return Ok(());
    }
    match listing.state {
        HubState::Down => println!("hub: down"),
        HubState::Stale => println!("hub: stale"),
        HubState::Up => match listing.pid {
            Some(pid) => println!("hub: pid {pid}"),
            None => println!("hub: up"),
        },
    }
    if let Some(ttl) = &window {
        println!("window: {}", window_with_cap(ttl, listing.max_cache_ms));
    }
    if !rows.is_empty() {
        println!();
        for row in rows {
            let cwd = crate::preexec::path_for_display(Path::new(&row.cwd));
            let path = crate::preexec::path_for_display(Path::new(&row.path));
            let ttl = format_ttl_left(row.ttl_left_ms);
            println!("{}  {cwd}  {path}  {ttl}", row.name);
        }
    }
    if !listing.tunnels.is_empty() {
        println!();
        for tunnel in &listing.tunnels {
            println!(
                "{}  {}  {}  {}",
                tunnel.port,
                tunnel.key,
                tunnel.uri,
                tunnel_state(tunnel)
            );
        }
    }
    Ok(())
}

pub(crate) fn phase_name(phase: TunnelPhase) -> &'static str {
    match phase {
        TunnelPhase::Starting => "starting",
        TunnelPhase::Ready => "ready",
        TunnelPhase::Failed => "failed",
    }
}

pub(crate) fn tunnel_state(tunnel: &TunnelRow) -> String {
    if tunnel.phase != TunnelPhase::Ready {
        return phase_name(tunnel.phase).to_string();
    }
    if tunnel.leases > 0 {
        return format!("in use ({})", tunnel.leases);
    }
    match tunnel.ttl_left_ms {
        Some(ms) => format_ttl_left(ms),
        None => "stopping".to_string(),
    }
}

fn forget_all() -> Result<()> {
    Report::new()
        .line(match hub::stop() {
            hub::HubStop::Stopped => "forgot all keys and tunnels".to_string(),
            hub::HubStop::Down => "hub: down".to_string(),
            hub::HubStop::Stale => "hub: stale".to_string(),
        })
        .print();
    Ok(())
}

fn forget_named(names: &[String]) -> Result<()> {
    let line = match hub::forget_names(names) {
        Some(()) if names.len() == 1 => format!("forgot {}", names[0]),
        Some(()) => format!("forgot {} keys", names.len()),
        None => match hub::probe().state {
            HubState::Stale => "hub: stale".to_string(),
            _ => "hub: down".to_string(),
        },
    };
    Report::new().line(line).print();
    Ok(())
}

pub(crate) fn rows_for_scope<'a>(
    rows: &'a [CacheRow],
    scope: Option<&str>,
    global: bool,
) -> Vec<&'a CacheRow> {
    rows.iter()
        .filter(|row| global || scope.is_some_and(|cwd| hub::path_related(&row.cwd, cwd)))
        .collect()
}

fn listing_json(
    listing: &CacheListing,
    window: Option<&str>,
    rows: &[&CacheRow],
    global: bool,
) -> Value {
    json!({
        "state": listing.state,
        "pid": listing.pid,
        "tickets": listing.tickets,
        "window": window,
        "max_cache_ms": listing.max_cache_ms,
        "global": global,
        "keys": rows.iter().map(|row| {
            json!({
                "name": row.name,
                "cwd": row.cwd,
                "path": row.path,
                "rule": row.rule,
                "when": row.when,
                "user": row.user,
                "ttl_left_ms": row.ttl_left_ms,
            })
        }).collect::<Vec<_>>(),
        "tunnels": listing.tunnels.iter().map(|tunnel| {
            json!({
                "key": tunnel.key,
                "uri": tunnel.uri,
                "port": tunnel.port,
                "leases": tunnel.leases,
                "ttl_left_ms": tunnel.ttl_left_ms,
                "phase": phase_name(tunnel.phase),
            })
        }).collect::<Vec<_>>(),
    })
}

/// `8h, capped to 1h by max_cache`: the hub never keeps a row past its cap.
pub(crate) fn window_with_cap(window: &str, cap_ms: Option<u32>) -> String {
    let Ok(RuleTtl::Window(duration)) = RuleTtl::parse(window) else {
        return window.to_string();
    };
    match cap_ms {
        Some(0) => format!("{window}, cache off by max_cache"),
        Some(cap) if crate::config::duration_to_ttl_ms(duration) > cap => {
            format!(
                "{window}, capped to {} by max_cache",
                format_duration_ms(cap)
            )
        }
        _ => window.to_string(),
    }
}

/// `1h`, `1h30m`, `45s`: the largest units that add up exactly.
pub(crate) fn format_duration_ms(ms: u32) -> String {
    let secs = ms / 1000;
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    let parts = [(h, "h"), (m, "m"), (s, "s")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, unit)| format!("{n}{unit}"))
        .collect::<String>();
    if parts.is_empty() {
        format!("{ms}ms")
    } else {
        parts
    }
}

pub(crate) fn format_ttl_left(ms: u32) -> String {
    let secs = ms / 1000;
    if secs >= 3600 {
        format!("{}h left", secs / 3600)
    } else if secs >= 60 {
        format!("{}m left", secs / 60)
    } else {
        format!("{secs}s left")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::CacheRow;

    fn row(name: &str, cwd: &str) -> CacheRow {
        CacheRow {
            name: name.into(),
            cwd: cwd.into(),
            path: "/proj/lade.yml".into(),
            rule: ".".into(),
            when: "always".into(),
            user: String::new(),
            ttl_left_ms: 60_000,
        }
    }

    #[test]
    fn default_list_keeps_this_cwd_and_children() {
        let rows = [
            row("HERE", "/proj"),
            row("CHILD", "/proj/src"),
            row("OTHER", "/other"),
        ];
        let got: Vec<_> = rows_for_scope(&rows, Some("/proj"), false)
            .iter()
            .map(|row| row.name.as_str())
            .collect();
        assert_eq!(got, ["HERE", "CHILD"]);
    }

    #[test]
    fn global_list_keeps_every_cwd() {
        let rows = [row("HERE", "/proj"), row("OTHER", "/other")];
        assert_eq!(rows_for_scope(&rows, Some("/proj"), true).len(), 2);
    }
}
