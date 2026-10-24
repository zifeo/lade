#![cfg(unix)]

use crate::preexec::Shell;
use crate::redact::Redactor;
use anyhow::Result;
use std::{
    fs::File,
    os::fd::{AsRawFd, OwnedFd},
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};

#[cfg(unix)]
pub fn run(
    shell: &Shell,
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

    let watch = crate::child_signals::ChildWatch::new();
    let mut child = super::prepare_child(shell, command, env, cwd)
        .stdin(Stdio::inherit())
        .stdout(Stdio::from(slave_out))
        .stderr(Stdio::from(slave_err))
        .spawn()?;
    watch.set_pid(child.id(), false);

    drop(slave);

    let _resize = setup_sigwinch_resize(&master);

    // Cloning the master fd decouples the redactor read loop from the
    // resize thread's own clone of `master`.
    let master_for_read = master.try_clone()?;
    let mut master_reader = File::from(master_for_read);
    let _ = redactor.stream(&mut master_reader, &mut std::io::stdout().lock());

    let status = child.wait()?;
    watch.clear_pid();
    drop(_resize);
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
fn setup_sigwinch_resize(master: &OwnedFd) -> ResizeWatch {
    use nix::sys::signal::{SigHandler, Signal};

    static SIGWINCH_PENDING: AtomicBool = AtomicBool::new(false);

    extern "C" fn handler(_: nix::libc::c_int) {
        SIGWINCH_PENDING.store(true, Ordering::Relaxed);
    }
    unsafe { nix::sys::signal::signal(Signal::SIGWINCH, SigHandler::Handler(handler)).ok() };

    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let thread = master.try_clone().ok().map(|fd| {
        std::thread::spawn(move || {
            while !stop_thread.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if SIGWINCH_PENDING.swap(false, Ordering::Relaxed) {
                    let mut ws: nix::libc::winsize = unsafe { std::mem::zeroed() };
                    if unsafe {
                        nix::libc::ioctl(nix::libc::STDOUT_FILENO, nix::libc::TIOCGWINSZ, &mut ws)
                    } == 0
                    {
                        unsafe { nix::libc::ioctl(fd.as_raw_fd(), nix::libc::TIOCSWINSZ, &ws) };
                    }
                }
            }
        })
    });
    ResizeWatch { stop, thread }
}

#[cfg(unix)]
struct ResizeWatch {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(unix)]
impl Drop for ResizeWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(unix)]
struct RawStdinGuard {
    original: Option<nix::sys::termios::Termios>,
}

#[cfg(unix)]
impl RawStdinGuard {
    fn enter() -> Self {
        use nix::sys::termios::{LocalFlags, SetArg, cfmakeraw, tcgetattr, tcsetattr};
        let stdin = std::io::stdin();
        let Ok(original) = tcgetattr(&stdin) else {
            return Self { original: None };
        };
        let mut raw = original.clone();
        cfmakeraw(&mut raw);
        // cfmakeraw clears ISIG. Keep Ctrl-C as SIGINT so ChildWatch stops the child.
        raw.local_flags |= LocalFlags::ISIG;
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
