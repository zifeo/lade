use anyhow::Result;

use super::upgrade;
use crate::context::InvocationContext;
use crate::exit_codes;
use crate::message_box::Report;
use crate::pretool;

pub(crate) async fn run_setup(
    ctx: &InvocationContext,
    slugs: &[&str],
    mode: crate::mise::PinMode,
) -> Result<()> {
    let shell = crate::preexec::maybe_bootstrap_setup_shell()?;
    let may_prompt = ctx.may_ask();
    let cwd = std::env::current_dir()?;
    let snap = crate::mise::scan(&cwd);
    if snap.project_git_root.is_none() {
        // § h528pmcm2.b@m4j6a
        let home_plane = snap.is_mise() && crate::config::at_user_home(&cwd);
        let skipped = if home_plane {
            "Pre-tool hooks are skipped. Packages extend the mise files in your home folder."
        } else {
            "Packages, locks, and pre-tool hooks are skipped."
        };
        let mut report = Report::new().heading("This folder is not a git repo.");
        if crate::preexec::ci_job() {
            report = report.dim(format!("CI. No shell wrap. {skipped}"));
        } else {
            report = report
                .dim(format!("Shell wrap only. {skipped}"))
                .dim("cd into a repo and run `lade setup`.");
        }
        report.print();
        if home_plane {
            require_lade_yaml()?;
            crate::mise::setup_pins(mode).await?;
        }
        // § end h528pmcm2.b
        let tool = pretool::install::setup(may_prompt, slugs)?;
        pretool::install::print_setup(&shell, &tool);
        upgrade::nudge_if_available();
        return Ok(());
    }
    require_lade_yaml()?;
    let saved = crate::global_config::GlobalConfig::resolved_user()?;
    let config = crate::config::LadeFile::build(cwd.clone())?;
    let show_packages = crate::mise::repo_needs_mise(&config, &saved);
    if show_packages {
        Report::new()
            .heading("packages")
            .dim("CLIs this repo wraps. Leave the folder and your usual ones come back.")
            .print();
    }
    {
        let _progress = if show_packages {
            crate::live_progress::begin_keep(ctx.stderr_is_terminal)
        } else {
            crate::live_progress::begin(ctx.stderr_is_terminal)
        };
        crate::mise::setup_pins(mode).await?;
        crate::mise::run_lifecycle_commands("setup").await?;
        crate::packages::run("setup").await?;
    }
    pretool::install::print_preexec(&shell);
    match pretool::install::plan(false, slugs)? {
        None => {
            pretool::install::print_pretool_intro(
                "not a git repo, pre-tool skipped",
                pretool::install::PRETOOL_SETUP_DIM,
            );
            pretool::install::print_pretool_rows(&pretool::install::setup(false, slugs)?, false);
        }
        Some(mut planned) => {
            pretool::install::print_pretool_intro(
                &planned.where_line(),
                pretool::install::PRETOOL_SETUP_DIM,
            );
            pretool::install::confirm_plan(&mut planned, may_prompt, slugs)?;
            let tool = pretool::install::commit(&planned)?;
            pretool::install::print_pretool_rows(&tool, false);
        }
    }
    upgrade::nudge_if_available();
    Ok(())
}

pub(crate) fn require_lade_yaml() -> Result<()> {
    // § y4zs3xgg4.c@1rmhg
    let cwd = std::env::current_dir()?;
    if let Err(e) = crate::config::LadeFile::build(cwd) {
        crate::config::report_load_error(&e);
        std::process::exit(exit_codes::FAILURE);
    }
    Ok(())
}
