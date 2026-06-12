use std::io::{IsTerminal, stderr};

pub const DEFAULT_WIDTH: usize = 80;
pub const MIN_WIDTH: usize = 40;
pub const MAX_WIDTH: usize = 120;

pub fn detect_width() -> usize {
    clamp_width(
        terminal_columns()
            .or_else(columns_env)
            .unwrap_or(DEFAULT_WIDTH),
    )
}

pub fn clamp_width(width: usize) -> usize {
    width.clamp(MIN_WIDTH, MAX_WIDTH)
}

pub fn columns_env() -> Option<usize> {
    std::env::var("COLUMNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .filter(|&cols| cols > 0)
}

#[cfg(unix)]
pub fn terminal_columns() -> Option<usize> {
    use std::mem::MaybeUninit;
    use std::os::unix::io::AsRawFd;

    let err = stderr();
    if !err.is_terminal() {
        return None;
    }

    let mut ws = MaybeUninit::<nix::libc::winsize>::uninit();
    let ret = unsafe { nix::libc::ioctl(err.as_raw_fd(), nix::libc::TIOCGWINSZ, ws.as_mut_ptr()) };
    if ret < 0 {
        return None;
    }

    let cols = unsafe { ws.assume_init() }.ws_col as usize;
    (cols > 0).then_some(cols)
}

#[cfg(not(unix))]
pub fn terminal_columns() -> Option<usize> {
    if !stderr().is_terminal() {
        return None;
    }
    columns_env()
}

pub fn colors_enabled() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    if std::env::var("TERM")
        .ok()
        .is_some_and(|term| term.eq_ignore_ascii_case("dumb"))
    {
        return false;
    }
    stderr().is_terminal()
}
