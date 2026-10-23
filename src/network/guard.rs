//! `lade tunnel-guard -- PROGRAM ARGS...` runs a tunnel provider and takes
//! its process group down once stdin closes. The Lade process that started
//! it holds the other end of that pipe, so the kernel closes it however
//! Lade ends, `SIGKILL` included, and no provider outlives its owner.

use nix::sys::signal::{Signal, killpg};
use nix::unistd::{getpgrp, setsid};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Stdio};

pub(crate) const GUARD_VERB: &str = "tunnel-guard";

/// The kill takes the guard's process group, so the guard leads its own
/// session: the supervisor already made it, and `setsid` covers a guard run
/// from a script.
pub fn run_guard(args: &[OsString]) -> ! {
    let _ = setsid();
    let Some((program, rest)) = args
        .split_first()
        .filter(|(dashes, _)| *dashes == "--")
        .and_then(|(_, command)| command.split_first())
    else {
        let _ = writeln!(
            std::io::stderr(),
            "usage: lade {GUARD_VERB} -- PROGRAM ARGS..."
        );
        std::process::exit(2);
    };
    let mut child = match Command::new(program)
        .args(rest)
        .stdin(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "{}: {e}", program.to_string_lossy());
            std::process::exit(127);
        }
    };
    std::thread::spawn(|| {
        let mut sink = [0u8; 64];
        while matches!(std::io::stdin().read(&mut sink), Ok(n) if n > 0) {}
        let _ = killpg(getpgrp(), Signal::SIGKILL);
    });
    let code = match child.wait() {
        Ok(status) => status
            .code()
            .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
        Err(_) => 1,
    };
    std::process::exit(code);
}
