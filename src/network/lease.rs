use anyhow::{Result, anyhow};

/// Start time of `pid` in µs since the epoch, `None` once it is gone. A pid
/// plus its start time names one process even after the pid is reused.
#[cfg(target_os = "macos")]
pub(crate) fn process_started(pid: u32) -> Option<u64> {
    // sysctl like `ps`, not proc_pidinfo: that refuses another user's
    // process, and under Terminal, iTerm or Ghostty the session leader is
    // root's `login`. libc has no `kinfo_proc`: it is 648 bytes and opens
    // with `p_starttime`, a `timeval` (i64 seconds, i32 µs).
    let mut mib = [
        libc::CTL_KERN,
        libc::KERN_PROC,
        libc::KERN_PROC_PID,
        i32::try_from(pid).ok()?,
    ];
    let mut info = [0u8; 648];
    let mut size = info.len();
    let rc = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            4,
            info.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size != info.len() {
        return None;
    }
    let secs = u64::try_from(i64::from_ne_bytes(info[0..8].try_into().ok()?)).ok()?;
    let micros = u64::try_from(i32::from_ne_bytes(info[8..12].try_into().ok()?)).ok()?;
    secs.checked_mul(1_000_000)?.checked_add(micros)
}

/// Start time of `pid` in clock ticks since boot, `None` once it is gone.
#[cfg(target_os = "linux")]
pub(crate) fn process_started(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // Field 22; the command in field 2 may hold spaces and parens.
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_whitespace().nth(19)?.parse().ok()
}

/// Who keeps a hub tunnel open. The hub drops the lease when that process
/// is gone, so a killed shell or harness never pins a tunnel.
pub(crate) fn lease_for(id: String, pid: u32) -> Result<crate::hub::Lease> {
    let started = process_started(pid).ok_or_else(|| anyhow!("process {pid} is gone"))?;
    Ok(crate::hub::Lease { id, pid, started })
}

/// `lade inject` and harness commands: this process.
pub(crate) fn own_lease() -> Result<crate::hub::Lease> {
    let pid = std::process::id();
    lease_for(format!("p{pid}"), pid)
}

/// Pre-exec: `lade set` exits right away, so the lease follows the
/// terminal session leader. Its parent is often a short-lived subshell.
pub(crate) fn shell_lease(ticket: &str) -> Result<crate::hub::Lease> {
    let sid = nix::unistd::getsid(None)?.as_raw();
    lease_for(ticket.to_string(), u32::try_from(sid)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_process_has_a_stable_start_time() {
        let pid = std::process::id();
        assert!(process_started(pid).is_some());
        assert_eq!(process_started(pid), process_started(pid));
    }

    #[test]
    fn reaped_child_has_no_start_time() {
        let mut child = std::process::Command::new("/bin/sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let pid = child.id();
        child.wait().unwrap();
        assert_eq!(process_started(pid), None);
    }

    #[test]
    fn another_users_process_has_a_start_time() {
        assert!(process_started(1).is_some());
    }
}
