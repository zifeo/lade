//! Who may talk to the hub, and who may read its memory. On macOS the
//! kernel names a client by its code directory hash (cdhash), checked
//! through the audit token of that exact process. With hardened runtime,
//! no other process of this user can attach to the hub.

use serde::Serialize;

/// Whether this binary's hub may cache secrets, and why: the hub runs this
/// same binary. No guard, no cache: commands then hydrate every time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum MemoryGuard {
    /// macOS: signed with hardened runtime, no debugger can attach.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    HardenedRuntime,
    /// Linux: the hub is not dumpable.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    NotDumpable,
    /// macOS debug build (tests, `cargo run`): cargo does not sign it.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    DebugBuild,
    /// No secret cached. `detail` says why, and how to fix it.
    Off { detail: String },
}

#[cfg(test)]
thread_local! {
    pub(super) static GUARD_OFF_FOR_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

pub fn memory_guard() -> MemoryGuard {
    #[cfg(test)]
    if GUARD_OFF_FOR_TEST.with(std::cell::Cell::get) {
        return MemoryGuard::Off {
            detail: "off for this test".to_string(),
        };
    }
    #[cfg(target_os = "macos")]
    {
        // § kgejwr97c.b@6b9tk
        if own_status().is_some_and(|flags| flags & CS_RUNTIME != 0) {
            return MemoryGuard::HardenedRuntime;
        }
        if cfg!(debug_assertions) {
            return MemoryGuard::DebugBuild;
        }
        let exe = std::env::current_exe()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|_| "lade".to_string());
        MemoryGuard::Off {
            detail: format!(
                "not signed with hardened runtime. Sign it once: \
                 codesign --force --sign - --options runtime {exe}"
            ),
        }
    }
    // The hub sets it itself at start: no signature, no privilege.
    #[cfg(target_os = "linux")]
    {
        // § kgejwr97c.a@ncvat
        MemoryGuard::NotDumpable
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    MemoryGuard::Off {
        detail: "no memory guard on this OS".to_string(),
    }
}

/// What the hub could not do to keep its memory to itself. Holds no heap
/// data, so [`lock_memory`] can run in a forked child.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Unguarded {
    what: &'static str,
    /// `None`: the call returned, but the kernel did not apply it.
    errno: Option<i32>,
}

impl Unguarded {
    fn last(what: &'static str) -> Self {
        Self {
            what,
            errno: std::io::Error::last_os_error().raw_os_error(),
        }
    }
}

impl std::fmt::Display for Unguarded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.errno {
            Some(errno) => write!(
                f,
                "the hub could not {}: {}",
                self.what,
                std::io::Error::from_raw_os_error(errno)
            ),
            None => write!(f, "the hub could not {}", self.what),
        }
    }
}

/// Run once by the hub: no core file, and on Linux no `ptrace` or
/// `/proc/<pid>/mem` from another process of this user. An error: the hub
/// must cache no secret.
pub(super) fn lock_memory() -> Result<(), Unguarded> {
    // § kgejwr97c.d@d45d8
    let none = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::setrlimit(libc::RLIMIT_CORE, &none) } != 0 {
        return Err(Unguarded::last("turn off its core file"));
    }
    // § end kgejwr97c.d
    #[cfg(target_os = "linux")]
    {
        // § kgejwr97c.a@7tac6
        const WHAT: &str = "make itself not dumpable";
        if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0 {
            return Err(Unguarded::last(WHAT));
        }
        if unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) } != 0 {
            return Err(Unguarded {
                what: WHAT,
                errno: None,
            });
        }
        // § end kgejwr97c.a
    }
    Ok(())
}

#[cfg(target_os = "macos")]
const CS_OPS_STATUS: libc::c_uint = 0;
#[cfg(target_os = "macos")]
const CS_OPS_CDHASH: libc::c_uint = 5;
#[cfg(target_os = "macos")]
const CS_VALID: u32 = 0x0000_0001;
#[cfg(target_os = "macos")]
const CS_RUNTIME: u32 = 0x0001_0000;

#[cfg(target_os = "macos")]
type AuditToken = [u32; 8];

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn csops(
        pid: libc::pid_t,
        ops: libc::c_uint,
        useraddr: *mut libc::c_void,
        usersize: libc::size_t,
    ) -> libc::c_int;
    fn csops_audittoken(
        pid: libc::pid_t,
        ops: libc::c_uint,
        useraddr: *mut libc::c_void,
        usersize: libc::size_t,
        token: *const AuditToken,
    ) -> libc::c_int;
}

#[cfg(target_os = "macos")]
fn own_status() -> Option<u32> {
    let mut flags = 0u32;
    let rc = unsafe { csops(libc::getpid(), CS_OPS_STATUS, (&raw mut flags).cast(), 4) };
    (rc == 0).then_some(flags)
}

/// `None` for an unsigned binary (an Intel `cargo build`): the hub then
/// knows a client by its file only.
#[cfg(target_os = "macos")]
pub(super) fn own_cdhash() -> Option<[u8; 20]> {
    let mut hash = [0u8; 20];
    let rc = unsafe { csops(libc::getpid(), CS_OPS_CDHASH, hash.as_mut_ptr().cast(), 20) };
    (rc == 0).then_some(hash)
}

/// The cdhash of the process at the other end, if its code is valid. The
/// audit token names that process: a pid reused or an `exec` since the
/// connect fails the call.
#[cfg(target_os = "macos")]
pub(super) fn peer_cdhash(stream: &impl std::os::fd::AsRawFd) -> Option<[u8; 20]> {
    // § e1s6kxps0.b@2n2ny
    const SOL_LOCAL: libc::c_int = 0;
    const LOCAL_PEERTOKEN: libc::c_int = 0x006;
    let mut token: AuditToken = [0; 8];
    let mut len = std::mem::size_of::<AuditToken>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            SOL_LOCAL,
            LOCAL_PEERTOKEN,
            (&raw mut token).cast(),
            &mut len,
        )
    };
    if rc != 0 {
        return None;
    }
    let pid = token[5] as libc::pid_t;
    let mut flags = 0u32;
    let mut hash = [0u8; 20];
    let status =
        unsafe { csops_audittoken(pid, CS_OPS_STATUS, (&raw mut flags).cast(), 4, &token) };
    let cdhash =
        unsafe { csops_audittoken(pid, CS_OPS_CDHASH, hash.as_mut_ptr().cast(), 20, &token) };
    (status == 0 && flags & CS_VALID != 0 && cdhash == 0).then_some(hash)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn a_process_knows_its_own_cdhash() {
        let flags = own_status().expect("code signing status");
        assert_ne!(flags & CS_VALID, 0);
        #[cfg(target_arch = "aarch64")]
        assert!(own_cdhash().is_some(), "arm64 binaries are linker-signed");
    }

    #[test]
    fn the_other_end_of_a_socket_has_the_same_cdhash() {
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        assert_eq!(peer_cdhash(&a), own_cdhash());
    }
}

#[cfg(test)]
mod lock_tests {
    use super::*;

    /// In a forked child: the limits it sets would otherwise hold for every
    /// test of this process. The child only makes syscalls, so a lock
    /// another thread held at the fork cannot block it.
    #[test]
    fn a_locked_hub_has_no_core_file_and_is_not_dumpable() {
        let pid = unsafe { libc::fork() };
        assert!(pid >= 0, "fork");
        if pid == 0 {
            let mut core = libc::rlimit {
                rlim_cur: 1,
                rlim_max: 1,
            };
            let locked = lock_memory().is_ok();
            let no_core = unsafe { libc::getrlimit(libc::RLIMIT_CORE, &mut core) } == 0
                && core.rlim_cur == 0
                && core.rlim_max == 0;
            #[cfg(target_os = "linux")]
            let not_dumpable = unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) } == 0;
            #[cfg(not(target_os = "linux"))]
            let not_dumpable = true;
            let code =
                i32::from(!locked) | i32::from(!no_core) << 1 | i32::from(!not_dumpable) << 2;
            unsafe { libc::_exit(code) };
        }
        let mut status = 0;
        assert_eq!(unsafe { libc::waitpid(pid, &mut status, 0) }, pid);
        assert!(libc::WIFEXITED(status), "child status {status}");
        assert_eq!(
            libc::WEXITSTATUS(status),
            0,
            "bit 0: lock_memory failed, 1: core limit not 0, 2: still dumpable"
        );
    }

    #[test]
    fn an_unguarded_hub_says_what_it_could_not_do() {
        let why = Unguarded {
            what: "make itself not dumpable",
            errno: Some(libc::EPERM),
        };
        let text = why.to_string();
        assert!(
            text.starts_with("the hub could not make itself not dumpable: "),
            "{text}"
        );
    }
}
