use std::path::Path;

use chrono::{DateTime, Local, Utc};

use crate::pretool;

use super::StatusReport;

pub(super) fn print_human(report: &StatusReport) {
    let v = &report.version;
    match &v.check_error {
        Some(err) => println!("lade version: {} ({err})", v.current),
        None => {
            println!("lade version: {}", v.current);
            match (&v.latest, v.update_available) {
                (Some(latest), true) => {
                    println!("  latest: {latest} (update available. run `lade upgrade`)")
                }
                (Some(latest), false) => println!("  latest: {latest} (up to date)"),
                (None, _) => match v.last_check {
                    Some(at) => println!("  latest: ({})", format_tried_at(at, Local::now())),
                    None => println!("  latest: (not checked yet)"),
                },
            }
        }
    }

    println!(
        "global config: {}",
        display_path(&report.global_config.path)
    );
    match &report.global_config.user {
        Some(user) => println!("  user: {user}"),
        None => println!("  user: (OS default)"),
    }

    println!("pre-exec (this shell) ({})", report.hooks.preexec.shell);
    println!("  profile: {}", display_path(&report.hooks.preexec.profile));
    if report.hooks.preexec.installed {
        println!("  installed: yes");
    } else {
        println!("  installed: no (run `lade install`)");
    }
    match &report.hooks.preexec.inject_startup_skipped {
        Some(name) => println!("  inject wrap: skips startup files ({name} present)"),
        None => println!("  inject wrap: skips startup files (bashrc, zshenv, fish config)"),
    }

    println!("pre-tool (agents)");
    print_pretool_line("Cursor this machine", &report.hooks.pretool.cursor.global);
    print_pretool_line("Cursor this repo", &report.hooks.pretool.cursor.project);
    print_pretool_line(
        "Claude Code this machine",
        &report.hooks.pretool.claude.global,
    );
    print_pretool_line(
        "Claude Code this repo",
        &report.hooks.pretool.claude.project,
    );
    print_pretool_line("Codex this machine", &report.hooks.pretool.codex.global);
    print_pretool_line("Codex this repo", &report.hooks.pretool.codex.project);
    print_pretool_line(
        "OpenCode this machine",
        &report.hooks.pretool.opencode.global,
    );
    print_pretool_line("OpenCode this repo", &report.hooks.pretool.opencode.project);

    println!("skills");
    print_pretool_line("Cursor this machine", &report.skills.cursor.global);
    print_pretool_line("Cursor this repo", &report.skills.cursor.project);
    print_pretool_line("Claude Code this machine", &report.skills.claude.global);
    print_pretool_line("Claude Code this repo", &report.skills.claude.project);
    print_pretool_line("Codex this machine", &report.skills.codex.global);
    print_pretool_line("Codex this repo", &report.skills.codex.project);
    print_pretool_line("OpenCode this machine", &report.skills.opencode.global);
    print_pretool_line("OpenCode this repo", &report.skills.opencode.project);
    if has_stale_pretool(report) {
        println!("  drift: run `lade install` to refresh stale hooks and skills");
    }

    let pc = &report.project_config;
    println!(
        "log: {} ({} events, {})",
        display_path(&report.log.path),
        report.log.events,
        format_bytes(report.log.bytes)
    );
    if let Some(err) = &pc.error {
        println!("project config: error");
        println!("  {err}");
        return;
    }
    println!("project config: ok ({} rules)", pc.rule_count);
    if pc.providers.is_empty() && pc.vault_clis.checked.is_empty() {
        println!("providers: (none referenced in lade.yml)");
        return;
    }
    println!("providers:");
    for provider in &pc.providers {
        match provider.transport.as_str() {
            "sdk" => {
                let version = provider.version.as_deref().unwrap_or(lade_sdk::VERSION);
                println!(
                    "  {}: sdk {version}, {}",
                    provider.scheme, provider.batch_unit
                );
            }
            _ => {
                match pc
                    .vault_clis
                    .warnings
                    .iter()
                    .find(|w| w.name == provider.name)
                {
                    Some(w) => println!(
                        "  {}: cli ({} < {}, {})",
                        provider.scheme, w.found, w.min, w.install_url
                    ),
                    None => println!("  {}: cli, {}", provider.scheme, provider.batch_unit),
                }
            }
        }
    }
    for w in &pc.vault_clis.warnings {
        if pc.providers.iter().any(|p| p.name == w.name) {
            continue;
        }
        println!("  {} {} < {} ({})", w.name, w.found, w.min, w.install_url);
    }
}

fn has_stale_pretool(report: &StatusReport) -> bool {
    fn stale(location: &pretool::install::HookLocation) -> bool {
        location.installed && !location.current
    }
    fn agent(status: &pretool::install::PretoolAgentStatus) -> bool {
        stale(&status.global) || stale(&status.project)
    }
    let hooks = &report.hooks.pretool;
    let skills = &report.skills;
    agent(&hooks.cursor)
        || agent(&hooks.claude)
        || agent(&hooks.codex)
        || agent(&hooks.opencode)
        || agent(&skills.cursor)
        || agent(&skills.claude)
        || agent(&skills.codex)
        || agent(&skills.opencode)
}

pub(super) fn pretool_flag(installed: bool, current: bool) -> &'static str {
    match (installed, current) {
        (true, true) => "yes",
        (true, false) => "yes (stale)",
        (false, _) => "no",
    }
}

fn print_pretool_line(label: &str, location: &pretool::install::HookLocation) {
    let flag = pretool_flag(location.installed, location.current);
    println!("  {label}: {} ({flag})", display_path(&location.path));
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

pub(super) fn format_bytes(n: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    if n < 1024 {
        format!("{n} B")
    } else if (n as f64) < MB {
        format!("{:.1} KB", n as f64 / KB)
    } else if (n as f64) < GB {
        format!("{:.1} MB", n as f64 / MB)
    } else {
        format!("{:.1} GB", n as f64 / GB)
    }
}

pub(super) fn format_tried_at(checked_at: DateTime<Utc>, now: DateTime<Local>) -> String {
    let local = checked_at.with_timezone(&Local);
    let today = now.date_naive();
    let day = local.date_naive();
    let time = local.format("%H:%M");
    if day == today {
        format!("tried today at {time}")
    } else if day.checked_add_days(chrono::Days::new(1)) == Some(today) {
        format!("tried yesterday at {time}")
    } else {
        format!("tried {} at {time}", local.format("%d %b %Y"))
    }
}
