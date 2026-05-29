use anyhow::Result;
use std::io::{self, BufRead, Write};

use crate::message_box::MessageBox;

pub fn prompt(disclaimers: &[String]) -> Result<()> {
    if disclaimers.is_empty() {
        return Ok(());
    }

    MessageBox::new()
        .line("Are you sure you want to proceed?")
        .paragraphs(disclaimers.iter().map(String::as_str))
        .print_stderr();

    eprint!("Type \"yes\" to continue (Ctrl+C to cancel): ");
    io::stderr().flush()?;

    let input = read_confirmation()?;
    if input.trim() != "yes" {
        abort_prompt(1);
    }

    Ok(())
}

fn read_confirmation() -> Result<String> {
    #[cfg(unix)]
    {
        let _guard = SigintGuard::install()?;
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input)?;
        Ok(input)
    }
    #[cfg(not(unix))]
    {
        let mut input = String::new();
        io::stdin().lock().read_line(&mut input)?;
        Ok(input)
    }
}

fn abort_prompt(code: i32) -> ! {
    eprintln!();
    eprintln!("Not injecting secrets, aborting.");
    std::process::exit(code);
}

#[cfg(unix)]
mod sigint {
    use nix::sys::signal::{self, SaFlags, SigAction, SigHandler, SigSet, Signal};

    const ABORT_MSG: &[u8] = b"\nNot injecting secrets, aborting.\n";

    /// Async-signal-safe: macOS often keeps `read_line` blocked unless we exit from the handler.
    extern "C" fn handler(_: i32) {
        unsafe {
            nix::libc::write(
                nix::libc::STDERR_FILENO,
                ABORT_MSG.as_ptr().cast(),
                ABORT_MSG.len(),
            );
            nix::libc::_exit(130);
        }
    }

    pub struct SigintGuard {
        old: SigAction,
    }

    impl SigintGuard {
        pub fn install() -> nix::Result<Self> {
            let action = SigAction::new(
                SigHandler::Handler(handler),
                SaFlags::empty(),
                SigSet::empty(),
            );
            let old = unsafe { signal::sigaction(Signal::SIGINT, &action)? };
            Ok(Self { old })
        }
    }

    impl Drop for SigintGuard {
        fn drop(&mut self) {
            let _ = unsafe { signal::sigaction(Signal::SIGINT, &self.old) };
        }
    }
}

#[cfg(unix)]
use sigint::SigintGuard;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_disclaimers_is_noop() {
        prompt(&[]).unwrap();
    }
}
