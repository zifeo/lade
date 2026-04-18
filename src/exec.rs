use std::{collections::HashMap, io::IsTerminal, path::Path, sync::Arc};

use anyhow::Result;

use crate::redact::Redactor;

pub fn run(
    shell: &str,
    command: &str,
    env: HashMap<String, String>,
    cwd: &Path,
    redactor: Option<Redactor>,
) -> Result<i32> {
    let Some(redactor) = redactor else {
        return run_plain(shell, command, env, cwd);
    };
    let redactor = Arc::new(redactor);
    if std::io::stdout().is_terminal() {
        run_pty(shell, command, env, cwd, redactor)
    } else {
        run_piped(shell, command, env, cwd, redactor)
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

fn run_pty(
    shell: &str,
    command: &str,
    env: HashMap<String, String>,
    cwd: &Path,
    redactor: Arc<Redactor>,
) -> Result<i32> {
    use portable_pty::{CommandBuilder, PtyPair, native_pty_system};

    let PtyPair { master, slave } = native_pty_system().openpty(pty_size())?;

    let reader = master.try_clone_reader()?;
    let mut writer = master.take_writer()?;

    let mut cmd = CommandBuilder::new(shell);
    cmd.arg("-c");
    cmd.arg(command);
    cmd.cwd(cwd);
    for (k, v) in std::env::vars() {
        cmd.env(k, v);
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = slave.spawn_command(cmd)?;
    drop(slave);

    #[cfg(unix)]
    setup_sigwinch_resize(&*master);

    // Errors are expected when the child exits before consuming all input.
    std::thread::spawn(move || {
        std::io::copy(&mut std::io::stdin().lock(), &mut writer).ok();
    });

    redactor.stream(reader, &mut std::io::stdout().lock())?;

    Ok(child.wait()?.exit_code() as i32)
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

fn pty_size() -> portable_pty::PtySize {
    #[cfg(unix)]
    {
        let mut ws: nix::libc::winsize = unsafe { std::mem::zeroed() };
        if unsafe { nix::libc::ioctl(nix::libc::STDOUT_FILENO, nix::libc::TIOCGWINSZ, &mut ws) }
            == 0
            && ws.ws_row > 0
            && ws.ws_col > 0
        {
            return portable_pty::PtySize {
                rows: ws.ws_row,
                cols: ws.ws_col,
                pixel_width: ws.ws_xpixel,
                pixel_height: ws.ws_ypixel,
            };
        }
    }
    portable_pty::PtySize::default()
}

// Propagate terminal resize to the PTY by polling an atomic flag set by the
// signal handler. Polling avoids the async-signal-safety constraints of calling
// ioctl directly from the handler. The raw fd is used instead of keeping the
// master behind a Mutex because resize only needs &self and TIOCSWINSZ is
// safe to call concurrently; a harmless EBADF occurs if the master is already
// closed when the thread wakes.
#[cfg(unix)]
fn setup_sigwinch_resize(master: &(dyn portable_pty::MasterPty + Send)) {
    use nix::sys::signal::{SigHandler, Signal};
    use std::sync::atomic::{AtomicBool, Ordering};

    static SIGWINCH_PENDING: AtomicBool = AtomicBool::new(false);

    extern "C" fn handler(_: nix::libc::c_int) {
        SIGWINCH_PENDING.store(true, Ordering::Relaxed);
    }
    unsafe { nix::sys::signal::signal(Signal::SIGWINCH, SigHandler::Handler(handler)).ok() };

    let Some(master_fd) = master.as_raw_fd() else {
        return;
    };
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
