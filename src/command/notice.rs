//! Boxes in a human shell: a newer Lade, and hooks that need the user. A
//! command that talks to the hub anyway may get the hourly slot, which
//! covers the release and every harness. Starting a harness checks its own
//! hooks every time, since it is about to run them. The hub holds the
//! version ledger, so nothing here touches the network or `config.json`.

use self_update::cargo_crate_version;

use crate::hub::VersionLedger;
use crate::message_box::MessageBox;

use super::upgrade::VersionStatus;

/// Whether `lade set` asks the hub for the hourly slot. Its stdout is the
/// shell protocol, so a notice goes on stderr, for a human at a terminal.
pub(crate) fn shell_notice(ctx: &crate::context::InvocationContext) -> bool {
    // § jpm1fzdj4.a@7j33e
    ctx.audience == crate::config::Audience::Human && ctx.stderr_is_terminal
}

/// `slot`: the hub handed this shell the hourly slot.
pub(crate) fn on_shell_use(command: &str, slot: Option<VersionLedger>) {
    let (release, hooks) = match slot {
        // § khcwz2ec3.c@s7ygp
        Some(version) => (
            VersionStatus::from_ledger(cargo_crate_version!(), Some(version)).release_line(),
            crate::pretool::install::refresh_and_report(),
        ),
        None => (
            None,
            crate::pretool::install::refresh_and_report_launch(command),
        ),
    };
    let hooks = match hooks {
        Ok(lines) => lines,
        Err(error) => vec![format!("Lade could not check the harness hooks: {error:#}")],
    };
    if let Some(message) = notice_box(release, hooks) {
        message.print_stderr();
    }
}

/// A pre-tool wrap that uses the hub may get the same hourly slot. The
/// agent cannot upgrade Lade or fix a hook, so it is asked to relay.
pub(crate) fn on_agent_use() {
    let Some(version) = crate::hub::notice_slot() else {
        return;
    };
    let release = VersionStatus::from_ledger(cargo_crate_version!(), Some(version)).release_line();
    // § khcwz2ec3.c@975ek
    let hooks = crate::pretool::install::refresh_and_report()
        .unwrap_or_else(|error| vec![format!("Lade could not check the harness hooks: {error:#}")]);
    if let Some(message) = notice_box(release, hooks) {
        message.print_stderr();
    }
}

fn notice_box(release: Option<String>, hooks: Vec<String>) -> Option<MessageBox> {
    // § jpm1fzdj4.c@a8fnt
    if release.is_none() && hooks.is_empty() {
        return None;
    }
    let mut message = if hooks.is_empty() {
        MessageBox::new().info()
    } else {
        MessageBox::new().warning()
    };
    if crate::message_box::terse() {
        // § jpm1fzdj4.b@gr9sh
        message = message.line("Tell the human:");
    }
    if let Some(line) = release {
        message = message.line(line).line("Run `lade upgrade` to install it.");
    }
    for line in hooks {
        message = message.paragraph(line);
    }
    Some(message)
}

#[cfg(test)]
mod tests {
    use super::notice_box;

    #[test]
    fn nothing_to_say_prints_nothing() {
        assert!(notice_box(None, Vec::new()).is_none());
    }

    #[test]
    fn only_a_human_at_a_terminal_asks_for_the_slot() {
        use crate::audience::{UiMode, Via};
        use crate::config::Audience;
        let ctx = |audience, stderr_is_terminal| crate::context::InvocationContext {
            via: Via::Preexec,
            audience,
            mode: UiMode::Interactive,
            ticket_id: None,
            stdin_is_terminal: true,
            stdout_is_terminal: false,
            stderr_is_terminal,
        };
        assert!(super::shell_notice(&ctx(Audience::Human, true)));
        assert!(!super::shell_notice(&ctx(Audience::Human, false)));
        assert!(!super::shell_notice(&ctx(Audience::Agent, true)));
    }

    #[test]
    fn release_or_hooks_make_a_box() {
        assert!(notice_box(Some("New Lade release: 0.1.0 → 0.2.0.".into()), Vec::new()).is_some());
        assert!(notice_box(None, vec!["Cursor (this repo): ...".into()]).is_some());
    }
}
