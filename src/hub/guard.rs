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
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    Off { detail: String },
}

pub fn memory_guard() -> MemoryGuard {
    #[cfg(target_os = "macos")]
    {
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
        MemoryGuard::NotDumpable
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    MemoryGuard::Off {
        detail: "no memory guard on this OS".to_string(),
    }
}

/// Run once by the hub: no core file, and on Linux no `ptrace` or
/// `/proc/<pid>/mem` from another process of this user.
pub(super) fn lock_memory() {
    let none = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    unsafe { libc::setrlimit(libc::RLIMIT_CORE, &none) };
    #[cfg(target_os = "linux")]
    unsafe {
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
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
