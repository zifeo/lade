//! A provider CLI runs in its own process group, so a timeout kills what it
//! started. A terminal lets only its foreground group read: a CLI in the
//! background stops on `SIGTTIN` the moment it asks. While one runs, Lade
//! lends it the terminal, so `bw unlock`, an MFA code, or `sh://read` can
//! ask and get the answer. Providers run in parallel: the others wait in
//! line, and each gets the terminal, and a `SIGCONT`, when it is free.

use std::sync::{Mutex, OnceLock};

struct Line {
    owner: Option<libc::pid_t>,
    waiting: Vec<libc::pid_t>,
}

static LINE: Mutex<Line> = Mutex::new(Line {
    owner: None,
    waiting: Vec::new(),
});

/// Run around each lend: the caller stops reading the terminal itself and
/// gives it its usual modes back, then takes both up again.
struct Hooks {
    before: fn(),
    after: fn(),
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();

pub fn set_hooks(before_lend: fn(), after_lend: fn()) {
    let _ = HOOKS.set(Hooks {
        before: before_lend,
        after: after_lend,
    });
}

/// True while a provider holds the terminal: Lade draws nothing on it.
pub fn lent() -> bool {
    LINE.lock().is_ok_and(|line| line.owner.is_some())
}

/// Gives the terminal on or back when dropped, before a timeout kills the
/// group.
pub(crate) struct Lease(Option<libc::pid_t>);

impl Lease {
    /// Only when stdin is a terminal Lade holds in the foreground.
    pub(crate) fn take(pid: Option<u32>) -> Self {
        let Some(group) = pid.and_then(|pid| libc::pid_t::try_from(pid).ok()) else {
            return Self(None);
        };
        let Ok(mut line) = LINE.lock() else {
            return Self(None);
        };
        if line.owner.is_some() {
            line.waiting.push(group);
            return Self(Some(group));
        }
        // SAFETY: plain queries on fd 0 and this process.
        if unsafe { libc::isatty(0) != 1 || libc::tcgetpgrp(0) != libc::getpgrp() } {
            return Self(None);
        }
        if let Some(hooks) = HOOKS.get() {
            (hooks.before)();
        }
        give(&mut line, group);
        Self(Some(group))
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        let (Some(group), Ok(mut line)) = (self.0, LINE.lock()) else {
            return;
        };
        if line.owner != Some(group) {
            line.waiting.retain(|waiting| *waiting != group);
            return;
        }
        if !line.waiting.is_empty() {
            let next = line.waiting.remove(0);
            give(&mut line, next);
            return;
        }
        // SAFETY: Lade is a background group here, and taking the terminal
        // back from one raises SIGTTOU, which would stop Lade.
        unsafe {
            let previous = libc::signal(libc::SIGTTOU, libc::SIG_IGN);
            libc::tcsetpgrp(0, libc::getpgrp());
            libc::signal(libc::SIGTTOU, previous);
        }
        line.owner = None;
        if let Some(hooks) = HOOKS.get() {
            (hooks.after)();
        }
    }
}

fn give(line: &mut Line, group: libc::pid_t) {
    // SAFETY: `group` is a provider's own group. Handing the terminal on
    // from the background needs SIGTTOU ignored; SIGCONT resumes a CLI that
    // read before its turn.
    unsafe {
        let previous = libc::signal(libc::SIGTTOU, libc::SIG_IGN);
        libc::tcsetpgrp(0, group);
        libc::signal(libc::SIGTTOU, previous);
        libc::killpg(group, libc::SIGCONT);
    }
    line.owner = Some(group);
}
