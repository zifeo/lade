use std::{collections::HashMap, io::IsTerminal, path::Path, sync::Arc};

use anyhow::Result;

use crate::redact::Redactor;

#[derive(Debug, PartialEq, Eq)]
enum Mode {
    Plain,
    Pty,
    Piped,
}

fn select_mode(has_redactor: bool, stdin_tty: bool, stdout_tty: bool) -> Mode {
    if !has_redactor {
        return Mode::Plain;
    }
    // The PTY path keeps the child's stdin pointed at the real TTY (no
    // forwarder thread, no inner-PTY echo loop). It is worth taking only
    // when stdout is a TTY too — otherwise piping is simpler and just as
    // correct.
    if stdin_tty && stdout_tty {
        Mode::Pty
    } else {
        Mode::Piped
    }
}

pub fn run(
    shell: &str,
    command: &str,
    env: HashMap<String, String>,
    cwd: &Path,
    redactor: Option<Redactor>,
) -> Result<i32> {
    let mode = select_mode(
        redactor.is_some(),
        std::io::stdin().is_terminal(),
        std::io::stdout().is_terminal(),
    );
    match mode {
        Mode::Plain => run_plain(shell, command, env, cwd),
        Mode::Pty => run_pty(shell, command, env, cwd, Arc::new(redactor.unwrap())),
        Mode::Piped => run_piped(shell, command, env, cwd, Arc::new(redactor.unwrap())),
    }
}

fn run_plain(shell: &str, command: &str, env: HashMap<String, String>, cwd: &Path) -> Result<i32> {
    // Seed with the parent's env explicitly: on Windows, Rust's Command resolves
    // the executable using the child's env PATH, and passing std::env::vars()
    // keeps PATH order as-is so `bash` resolves to Git Bash before WSL's stub.
    let status = std::process::Command::new(shell)
        .args(["-c", command])
        .current_dir(cwd)
        .envs(std::env::vars())
        .envs(env)
        .status()?;
    Ok(status.code().unwrap_or(1))
}

#[cfg(not(unix))]
fn run_pty(
    shell: &str,
    command: &str,
    env: HashMap<String, String>,
    cwd: &Path,
    redactor: Arc<Redactor>,
) -> Result<i32> {
    // Non-Unix platforms fall back to the piped path; the hybrid PTY
    // design below relies on Unix-only nix primitives.
    run_piped(shell, command, env, cwd, redactor)
}

// Give the child a PTY for stdout/stderr (so tofu-like tools keep color and
// isatty-gated UI) while leaving stdin attached to the real TTY. The child
// reads user input directly from the controlling terminal — lade does not
// forward stdin through an inner PTY master, which is what used to produce
// the `^[]11;rgb:...^[\^[[;R` echo of OSC 11 / CPR responses.
#[cfg(unix)]
fn run_pty(
    shell: &str,
    command: &str,
    env: HashMap<String, String>,
    cwd: &Path,
    redactor: Arc<Redactor>,
) -> Result<i32> {
    use nix::pty::{OpenptyResult, openpty};
    use std::fs::File;
    use std::os::fd::{AsRawFd, OwnedFd};
    use std::process::{Command, Stdio};

    let size = pty_winsize();
    let OpenptyResult { master, slave } = openpty(&size, None)?;

    let slave_out: OwnedFd = slave.try_clone()?;
    let slave_err: OwnedFd = slave.try_clone()?;

    // Putting the real stdin TTY in raw mode before spawn means neither the
    // kernel line discipline on lade's end nor the child (which inherits
    // stdin) sees cooked-mode echo of terminal query responses arriving on
    // stdin. Interactive prompts keep working because programs like tofu
    // manage their own termios for reading a line.
    let _raw_guard = RawStdinGuard::enter();

    let mut child = Command::new(shell)
        .args(["-c", command])
        .current_dir(cwd)
        .envs(std::env::vars())
        .envs(env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::from(slave_out))
        .stderr(Stdio::from(slave_err))
        .spawn()?;

    let master_fd = master.as_raw_fd();
    drop(slave);

    setup_sigwinch_resize(master_fd);

    // Cloning the master fd decouples the redactor read loop from the
    // signal-handler resize thread's borrow of `master`.
    let master_for_read = master.try_clone()?;
    let mut master_reader = File::from(master_for_read);
    let _ = redactor.stream(&mut master_reader, &mut std::io::stdout().lock());

    let status = child.wait()?;
    drop(master);
    Ok(status.code().unwrap_or(1))
}

fn run_piped(
    shell: &str,
    command: &str,
    env: HashMap<String, String>,
    cwd: &Path,
    redactor: Arc<Redactor>,
) -> Result<i32> {
    use std::process::{Command, Stdio};

    // Inherit stdin directly so the child reads the parent's fd. A `piped`
    // stdin forwarded by a helper thread risks SIGPIPE (SIG_DFL at startup
    // kills the process) when the child exits before consuming forwarded
    // bytes, which is observable on Linux CI.
    let mut child = Command::new(shell)
        .args(["-c", command])
        .current_dir(cwd)
        .envs(std::env::vars())
        .envs(env)
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let child_stdout = child.stdout.take().unwrap();
    let child_stderr = child.stderr.take().unwrap();

    let redactor_stderr = Arc::clone(&redactor);
    let stdout_thread = std::thread::spawn(move || {
        redactor
            .stream(child_stdout, &mut std::io::stdout().lock())
            .ok();
    });
    let stderr_thread = std::thread::spawn(move || {
        redactor_stderr
            .stream(child_stderr, &mut std::io::stderr().lock())
            .ok();
    });

    let status = child.wait()?;
    stdout_thread.join().ok();
    stderr_thread.join().ok();

    Ok(status.code().unwrap_or(1))
}

#[cfg(unix)]
fn pty_winsize() -> nix::pty::Winsize {
    let mut ws: nix::libc::winsize = unsafe { std::mem::zeroed() };
    if unsafe { nix::libc::ioctl(nix::libc::STDOUT_FILENO, nix::libc::TIOCGWINSZ, &mut ws) } == 0
        && ws.ws_row > 0
        && ws.ws_col > 0
    {
        return nix::pty::Winsize {
            ws_row: ws.ws_row,
            ws_col: ws.ws_col,
            ws_xpixel: ws.ws_xpixel,
            ws_ypixel: ws.ws_ypixel,
        };
    }
    nix::pty::Winsize {
        ws_row: 24,
        ws_col: 80,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

// Propagate terminal resize to the inner PTY by polling an atomic flag set
// by the signal handler. Polling avoids the async-signal-safety constraints
// of calling ioctl from a handler. A harmless EBADF occurs if the master is
// already closed when the thread wakes.
#[cfg(unix)]
fn setup_sigwinch_resize(master_fd: std::os::fd::RawFd) {
    use nix::sys::signal::{SigHandler, Signal};
    use std::sync::atomic::{AtomicBool, Ordering};

    static SIGWINCH_PENDING: AtomicBool = AtomicBool::new(false);

    extern "C" fn handler(_: nix::libc::c_int) {
        SIGWINCH_PENDING.store(true, Ordering::Relaxed);
    }
    unsafe { nix::sys::signal::signal(Signal::SIGWINCH, SigHandler::Handler(handler)).ok() };

    std::thread::spawn(move || {
        loop {
            std::thread::sleep(std::time::Duration::from_millis(50));
            if SIGWINCH_PENDING.swap(false, Ordering::Relaxed) {
                let mut ws: nix::libc::winsize = unsafe { std::mem::zeroed() };
                if unsafe {
                    nix::libc::ioctl(nix::libc::STDOUT_FILENO, nix::libc::TIOCGWINSZ, &mut ws)
                } == 0
                {
                    unsafe { nix::libc::ioctl(master_fd, nix::libc::TIOCSWINSZ, &ws) };
                }
            }
        }
    });
}

// Put the real stdin TTY in raw mode for the lifetime of the guard. Raw mode
// disables canonical input, ECHO and ECHOCTL so terminal query responses
// (e.g. OSC 11, CPR) are no longer re-emitted by the kernel line discipline
// as `^[...` noise. Programs that need a cooked-mode read (e.g. tofu's
// confirmation prompt) manage their own termios for the read window.
#[cfg(unix)]
struct RawStdinGuard {
    original: Option<nix::sys::termios::Termios>,
}

#[cfg(unix)]
impl RawStdinGuard {
    fn enter() -> Self {
        use nix::sys::termios::{SetArg, cfmakeraw, tcgetattr, tcsetattr};
        let stdin = std::io::stdin();
        let Ok(original) = tcgetattr(&stdin) else {
            return Self { original: None };
        };
        let mut raw = original.clone();
        cfmakeraw(&mut raw);
        if tcsetattr(stdin, SetArg::TCSANOW, &raw).is_err() {
            return Self { original: None };
        }
        Self {
            original: Some(original),
        }
    }
}

#[cfg(unix)]
impl Drop for RawStdinGuard {
    fn drop(&mut self) {
        if let Some(original) = &self.original {
            let _ = nix::sys::termios::tcsetattr(
                std::io::stdin(),
                nix::sys::termios::SetArg::TCSANOW,
                original,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_mode_plain_without_redactor() {
        assert_eq!(select_mode(false, true, true), Mode::Plain);
        assert_eq!(select_mode(false, false, false), Mode::Plain);
    }

    #[test]
    fn select_mode_pty_only_when_both_tty() {
        assert_eq!(select_mode(true, true, true), Mode::Pty);
    }

    #[test]
    fn select_mode_piped_when_stdin_not_tty() {
        assert_eq!(select_mode(true, false, true), Mode::Piped);
    }

    #[test]
    fn select_mode_piped_when_stdout_not_tty() {
        assert_eq!(select_mode(true, true, false), Mode::Piped);
        assert_eq!(select_mode(true, false, false), Mode::Piped);
    }
}
