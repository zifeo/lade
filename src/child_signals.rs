use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

static CHILD_PID: AtomicI32 = AtomicI32::new(0);
static KILL_GROUP: AtomicBool = AtomicBool::new(false);
static STOP: AtomicBool = AtomicBool::new(false);
static STOP_SIGNAL: AtomicI32 = AtomicI32::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalAction {
    Ignore,
    Forward,
    Stop,
}

#[cfg(unix)]
pub fn signal_action(signal: nix::sys::signal::Signal) -> SignalAction {
    use nix::sys::signal::Signal::*;
    match signal {
        SIGHUP => SignalAction::Ignore,
        SIGINT | SIGTERM | SIGQUIT => SignalAction::Stop,
        SIGUSR1 | SIGUSR2 | SIGWINCH => SignalAction::Forward,
        _ => SignalAction::Ignore,
    }
}

#[derive(Clone, Copy)]
pub struct ChildWatch;

impl ChildWatch {
    pub fn new() -> Self {
        install();
        Self
    }

    pub fn set_pid(&self, pid: u32, group: bool) {
        KILL_GROUP.store(group, Ordering::Release);
        CHILD_PID.store(pid as i32, Ordering::Release);
        // A stop signal can arrive after clear_pid and before the next
        // set_pid. Replay it so a respawned child does not outlive the wrapper.
        #[cfg(unix)]
        unix::flush_pending();
    }

    pub fn clear_pid(&self) {
        CHILD_PID.store(0, Ordering::Release);
        KILL_GROUP.store(false, Ordering::Release);
    }

    pub fn stop_requested(&self) -> bool {
        STOP.load(Ordering::Acquire)
    }
}

#[cfg(all(test, unix))]
pub fn request_stop_for_test() {
    STOP.store(true, Ordering::Release);
    STOP_SIGNAL.store(nix::libc::SIGTERM, Ordering::Release);
}

#[cfg(test)]
pub fn reset_for_test() {
    CHILD_PID.store(0, Ordering::Release);
    KILL_GROUP.store(false, Ordering::Release);
    STOP.store(false, Ordering::Release);
    STOP_SIGNAL.store(0, Ordering::Release);
}

#[cfg(test)]
pub static SUPERVISE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub fn detach_session_tokio(command: &mut tokio::process::Command) {
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            nix::unistd::setsid().map_err(|e| std::io::Error::other(e.to_string()))?;
            Ok(())
        });
    }
    let _ = command;
}

fn install() {
    #[cfg(unix)]
    unix::install();
}

#[cfg(unix)]
mod unix {
    use super::{CHILD_PID, KILL_GROUP, STOP, STOP_SIGNAL, SignalAction, signal_action};
    use nix::libc::c_int;
    use nix::sys::signal::{
        SaFlags, SigAction, SigHandler, SigSet, SigmaskHow, Signal, kill, killpg, sigaction,
        sigprocmask,
    };
    use nix::unistd::Pid;
    use std::sync::Once;
    use std::sync::atomic::Ordering;

    static INSTALL: Once = Once::new();

    pub fn install() {
        INSTALL.call_once(|| {
            let mut unblock = SigSet::empty();
            for signal in [
                Signal::SIGHUP,
                Signal::SIGINT,
                Signal::SIGTERM,
                Signal::SIGQUIT,
                Signal::SIGUSR1,
                Signal::SIGUSR2,
                Signal::SIGWINCH,
            ] {
                let action = SigAction::new(
                    SigHandler::Handler(handle),
                    SaFlags::empty(),
                    SigSet::empty(),
                );
                let _ = unsafe { sigaction(signal, &action) };
                unblock.add(signal);
            }
            // Tokio's signal driver blocks these. Unblock so the handler runs
            // even when no child pid is registered yet (restart backoff).
            let _ = sigprocmask(SigmaskHow::SIG_UNBLOCK, Some(&unblock), None);
        });
    }

    extern "C" fn handle(raw: c_int) {
        let Ok(signal) = Signal::try_from(raw) else {
            return;
        };
        match signal_action(signal) {
            SignalAction::Ignore => {}
            SignalAction::Forward => deliver(signal, false),
            SignalAction::Stop => deliver(signal, true),
        }
    }

    fn deliver(signal: Signal, stop: bool) {
        if stop {
            STOP.store(true, Ordering::Release);
            STOP_SIGNAL.store(signal as c_int, Ordering::Release);
        }
        let pid = CHILD_PID.load(Ordering::Acquire);
        if pid > 0 {
            let pid = Pid::from_raw(pid);
            if KILL_GROUP.load(Ordering::Acquire) {
                let _ = killpg(pid, signal);
            } else {
                let _ = kill(pid, signal);
            }
        }
    }

    pub fn flush_pending() {
        if !STOP.load(Ordering::Acquire) {
            return;
        }
        let raw = STOP_SIGNAL.load(Ordering::Acquire);
        let Ok(signal) = Signal::try_from(raw) else {
            return;
        };
        deliver(signal, false);
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn hangup_is_ignored_stop_signals_end_the_wrapper() {
        use super::{SignalAction, signal_action};
        use nix::sys::signal::Signal::*;
        assert_eq!(signal_action(SIGHUP), SignalAction::Ignore);
        assert_eq!(signal_action(SIGINT), SignalAction::Stop);
        assert_eq!(signal_action(SIGTERM), SignalAction::Stop);
        assert_eq!(signal_action(SIGQUIT), SignalAction::Stop);
        assert_eq!(signal_action(SIGUSR1), SignalAction::Forward);
        assert_eq!(signal_action(SIGUSR2), SignalAction::Forward);
        assert_eq!(signal_action(SIGWINCH), SignalAction::Forward);
        assert_eq!(signal_action(SIGALRM), SignalAction::Ignore);
        assert_eq!(signal_action(SIGPIPE), SignalAction::Ignore);
        assert_eq!(signal_action(SIGCHLD), SignalAction::Ignore);
        assert_eq!(signal_action(SIGTSTP), SignalAction::Ignore);
    }
}
