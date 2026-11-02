use std::path::Path;

use anyhow::Result;

use super::agent::Agent;
use super::committed::{Committed, committed};
use super::paths::{hook_command, project_hook_command};
use super::write::Scope;

/// What one plane's hook file holds for a harness when it is current.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Want {
    /// Today's snapshot with this command.
    Command(String),
    /// A copy of the repo's committed Claude hooks.
    Copy(Committed),
}

/// `dest` is the git root. A committed Claude hook there is what the repo
/// plane copies, in place of the snapshot.
pub(super) fn want(agent: Agent, scope: Scope, dest: &Path) -> Result<Want> {
    Ok(match (scope, agent) {
        (Scope::User, _) => Want::Command(hook_command(agent)),
        (Scope::Project, Agent::Claude) => match committed(dest)? {
            Some(committed) => Want::Copy(committed),
            None => Want::Command(project_hook_command(agent)),
        },
        (Scope::Project, _) => Want::Command(project_hook_command(agent)),
    })
}

impl Want {
    pub(super) fn is_current(&self, agent: Agent, existing: &str) -> Result<bool> {
        match self {
            Want::Command(command) => agent.hook_uses_command(existing, command),
            Want::Copy(committed) => committed.is_copied_in(existing),
        }
    }

    pub(super) fn merged(&self, agent: Agent, existing: &str) -> Result<String> {
        match self {
            Want::Command(command) => agent.merge(existing, command),
            Want::Copy(committed) => committed.copy_into(existing),
        }
    }
}
