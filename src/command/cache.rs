use anyhow::Result;
use serde_json::{Value, json};
use std::path::Path;

use crate::args::{CacheAction, CacheCommand};
use crate::hub::{self, CacheListing, HubState};
use crate::message_box::Report;

pub fn run(opts: CacheCommand) -> Result<()> {
    match opts.action {
        None | Some(CacheAction::List) => print_list(opts.json),
        Some(CacheAction::Forget { names }) if names.is_empty() => forget_all(),
        Some(CacheAction::Forget { names }) => forget_named(&names),
    }
}

fn print_list(as_json: bool) -> Result<()> {
    let listing = hub::listing();
    if as_json {
        println!("{}", serde_json::to_string_pretty(&listing_json(&listing))?);
        return Ok(());
    }
    match listing.state {
        HubState::Off => println!("hub: off"),
        HubState::Down => println!("hub: down"),
        HubState::Stale => println!("hub: stale"),
        HubState::Up => match listing.pid {
            Some(pid) => println!("hub: pid {pid}"),
            None => println!("hub: up"),
        },
    }
    if listing.rows.is_empty() {
        return Ok(());
    }
    println!();
    for row in &listing.rows {
        let path = display_path(Path::new(&row.path));
        let ttl = format_ttl_left(row.ttl_left_ms);
        println!("{}  {}  {path}  {ttl}", row.name, row.rule);
    }
    Ok(())
}

fn forget_all() -> Result<()> {
    Report::new()
        .line(match hub::stop() {
            hub::HubStop::Stopped => "forgot all keys".to_string(),
            hub::HubStop::Down => "hub: down".to_string(),
            hub::HubStop::Off => "hub: off".to_string(),
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
            HubState::Off => "hub: off".to_string(),
            HubState::Stale => "hub: stale".to_string(),
            _ => "hub: down".to_string(),
        },
    };
    Report::new().line(line).print();
    Ok(())
}

fn listing_json(listing: &CacheListing) -> Value {
    json!({
        "state": listing.state,
        "pid": listing.pid,
        "tickets": listing.tickets,
        "keys": listing.rows.iter().map(|row| {
            json!({
                "name": row.name,
                "path": row.path,
                "rule": row.rule,
                "when": row.when,
                "user": row.user,
                "ttl_left_ms": row.ttl_left_ms,
            })
        }).collect::<Vec<_>>(),
    })
}

fn format_ttl_left(ms: u32) -> String {
    let secs = ms / 1000;
    if secs >= 3600 {
        format!("{}h left", secs / 3600)
    } else if secs >= 60 {
        format!("{}m left", secs / 60)
    } else {
        format!("{secs}s left")
    }
}

fn display_path(path: &Path) -> String {
    if let Some(home) = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf())
        && let Ok(stripped) = path.strip_prefix(&home)
    {
        if stripped.as_os_str().is_empty() {
            return "~".to_string();
        }
        return format!("~/{}", stripped.display());
    }
    path.display().to_string()
}
