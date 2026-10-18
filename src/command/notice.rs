//! Boxes in a human shell: a newer Lade, and hooks that need the user. A
//! command that talks to the hub anyway may get the hourly slot, which
//! covers the release and every harness. Starting a harness checks its own
//! hooks every time, since it is about to run them. The hub holds the
//! version ledger, so nothing here touches the network or `config.json`.

use self_update::cargo_crate_version;

use crate::hub::VersionLedger;
use crate::message_box::MessageBox;

use super::upgrade::VersionStatus;

/// `slot`: the hub handed this shell the hourly slot.
pub(crate) fn on_shell_use(command: &str, slot: Option<VersionLedger>) {
    let (release, hooks) = match slot {
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

fn notice_box(release: Option<String>, hooks: Vec<String>) -> Option<MessageBox> {
    if release.is_none() && hooks.is_empty() {
        return None;
    }
    let mut message = if hooks.is_empty() {
        MessageBox::new().info()
    } else {
        MessageBox::new().warning()
    };
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
    fn release_or_hooks_make_a_box() {
        assert!(notice_box(Some("New Lade release: 0.1.0 → 0.2.0.".into()), Vec::new()).is_some());
        assert!(notice_box(None, vec!["Cursor (this repo): ...".into()]).is_some());
    }
}
