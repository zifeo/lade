use anyhow::Result;

use super::{fetch_events, filter_opt, query_window, repo_filter};
use crate::args::UsageCommand;
use crate::catalog;
use crate::message_box;

pub fn run_usage(opts: UsageCommand) -> Result<()> {
    let (since, until, limit) =
        query_window(opts.since.as_deref(), opts.until.as_deref(), opts.limit)?;
    let cwd = std::env::current_dir()?;
    let audience = filter_opt(&opts.audience);
    let repo = repo_filter(opts.all, opts.path.as_deref(), &cwd);
    let events = fetch_events(
        &opts.source,
        since.as_ref(),
        until.as_ref(),
        None,
        audience,
        None,
        repo.as_deref(),
    )?;
    let mut rows = catalog::group_rules(&events);
    if let Some(n) = limit {
        rows.truncate(n);
    }
    if rows.is_empty() {
        if opts.json {
            println!("[]");
        } else {
            message_box::MessageBox::new()
                .info()
                .line("no events")
                .print_plain_stderr();
        }
        return Ok(());
    }
    if opts.json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        for row in rows {
            let tags = if row.tags.is_empty() {
                String::new()
            } else {
                format!("  {}", row.tags.join("+"))
            };
            println!("{}  {}  {}{tags}", row.count, row.rule, row.file);
        }
    }
    Ok(())
}
