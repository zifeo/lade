use std::fs::File;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;
use std::time::Duration;

use nix::errno::Errno;
use nix::sys::select::{FdSet, select};
use nix::sys::termios::{
    FlushArg, LocalFlags, SetArg, SpecialCharacterIndices, Termios, tcflush, tcgetattr, tcsetattr,
};
use tokio::sync::mpsc;

use super::FailureChoice;
use crate::config::Audience;
use crate::context::InvocationContext;

const CTRL_C: u8 = 0x03;

/// What a provider lending needs to pause the reader: see `pause`.
struct Paused {
    ctl: UnixStream,
    tty: File,
    saved: Termios,
    raw: Termios,
}

static PAUSE: Mutex<Option<Paused>> = Mutex::new(None);

/// Before a provider gets the terminal: no spinner left on it, and no key
/// read by Lade.
pub(crate) fn before_lend() {
    crate::provider_progress::wipe();
    pause();
}

pub(crate) fn after_lend() {
    resume();
}

/// A provider that asks on the terminal gets it cooked, and every key: the
/// reader stops reading until `resume`.
fn pause() {
    let Ok(guard) = PAUSE.lock() else {
        return;
    };
    let Some(paused) = guard.as_ref() else {
        return;
    };
    let mut ctl = &paused.ctl;
    if ctl.write_all(b"p").is_ok() {
        let _ = ctl.set_read_timeout(Some(Duration::from_millis(200)));
        let _ = ctl.read(&mut [0u8; 1]);
    }
    let _ = tcsetattr(&paused.tty, SetArg::TCSANOW, &paused.saved);
}

fn resume() {
    let Ok(guard) = PAUSE.lock() else {
        return;
    };
    let Some(paused) = guard.as_ref() else {
        return;
    };
    let _ = tcsetattr(&paused.tty, SetArg::TCSANOW, &paused.raw);
    let _ = (&paused.ctl).write_all(b"r");
}

/// `/dev/tty` without line editing, echo, or signals, and a thread reading
/// its keys. Ctrl-C must arrive as a byte: fish aborts its pre-exec hook on
/// SIGINT and runs the command anyway. Dropping it restores the terminal.
pub(super) struct Keys {
    tty: File,
    saved: Termios,
    keys: mpsc::UnboundedReceiver<u8>,
    /// Closing it wakes the reader, so the drop never waits on a read.
    wake: Option<UnixStream>,
    reader: Option<std::thread::JoinHandle<()>>,
}

impl Keys {
    /// Only for a human at a terminal.
    pub(super) fn enter(ctx: &InvocationContext) -> Option<Self> {
        if ctx.audience != Audience::Human || !ctx.stderr_is_terminal {
            return None;
        }
        let tty = File::open("/dev/tty").ok()?;
        let (wake, woken) = UnixStream::pair().ok()?;
        let reader_tty = tty.try_clone().ok()?;
        let saved = tcgetattr(&tty).ok()?;
        let mut raw = saved.clone();
        raw.local_flags &= !(LocalFlags::ICANON | LocalFlags::ECHO | LocalFlags::ISIG);
        raw.control_chars[SpecialCharacterIndices::VMIN as usize] = 1;
        raw.control_chars[SpecialCharacterIndices::VTIME as usize] = 0;
        tcsetattr(&tty, SetArg::TCSANOW, &raw).ok()?;
        *PAUSE.lock().ok()? = Some(Paused {
            ctl: wake.try_clone().ok()?,
            tty: tty.try_clone().ok()?,
            saved: saved.clone(),
            raw,
        });
        let (send, keys) = mpsc::unbounded_channel();
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; 64];
            let mut paused = false;
            loop {
                let mut ready = FdSet::new();
                if !paused {
                    ready.insert(reader_tty.as_fd());
                }
                ready.insert(woken.as_fd());
                match select(None, &mut ready, None, None, None) {
                    Ok(_) => {}
                    Err(Errno::EINTR) => continue,
                    Err(_) => return,
                }
                if ready.contains(woken.as_fd()) {
                    let mut byte = [0u8; 1];
                    match (&woken).read(&mut byte) {
                        Ok(1) if byte[0] == b'p' => {
                            paused = true;
                            let _ = (&woken).write_all(b"a");
                        }
                        Ok(1) => paused = false,
                        _ => return,
                    }
                    continue;
                }
                match (&reader_tty).read(&mut buf) {
                    Ok(0) => return,
                    Ok(read) => {
                        for key in &buf[..read] {
                            if send.send(*key).is_err() {
                                return;
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => return,
                }
            }
        });
        Some(Self {
            tty,
            saved,
            keys,
            wake: Some(wake),
            reader: Some(reader),
        })
    }

    /// Resolves on Ctrl-C. Other keys are dropped.
    pub(super) async fn ctrl_c(&mut self) {
        while let Some(key) = self.keys.recv().await {
            if key == CTRL_C {
                return;
            }
        }
        std::future::pending().await
    }

    /// Before a question: keys typed earlier are not an answer.
    pub(super) fn forget_typed(&mut self) {
        let _ = tcflush(&self.tty, FlushArg::TCIFLUSH);
        while self.keys.try_recv().is_ok() {}
    }

    /// Enter runs the command, Ctrl-C cancels it at once, and so does a
    /// terminal that goes away.
    pub(super) async fn answer(&mut self) -> FailureChoice {
        while let Some(key) = self.keys.recv().await {
            match key {
                b'\r' | b'\n' => return FailureChoice::RunAnyway,
                CTRL_C => return FailureChoice::Interrupted,
                _ => {}
            }
        }
        FailureChoice::Cancel
    }
}

impl Drop for Keys {
    fn drop(&mut self) {
        // Its copy of the wake socket must close too, or the reader never
        // sees the end.
        if let Ok(mut paused) = PAUSE.lock() {
            paused.take();
        }
        drop(self.wake.take());
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
        let _ = tcsetattr(&self.tty, SetArg::TCSANOW, &self.saved);
    }
}

/// For a `select!` arm when there may be no terminal.
pub(super) async fn ctrl_c(keys: &mut Option<Keys>) {
    match keys {
        Some(keys) => keys.ctrl_c().await,
        None => std::future::pending().await,
    }
}
