#![cfg(unix)]

use crate::redact::Redactor;
use anyhow::Result;
use std::{
    fs::File,
    os::fd::{AsRawFd, OwnedFd},
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
};

#[cfg(unix)]
pub fn run(
    shell: &str,
    command: &str,
    env: std::collections::HashMap<String, String>,
    cwd: &Path,
    redactor: Arc<Redactor>,
) -> Result<i32> {
    use nix::pty::{OpenptyResult, openpty};

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
