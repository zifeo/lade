use std::fmt::Write as _;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

/// Mapped image at `serve()`. The fd keeps the old vnode after an
/// in-place replace so the snapshot inode does not follow the new file.
pub struct ImageSnap {
    inode: (u64, u64),
    _hold: std::fs::File,
}

impl ImageSnap {
    pub fn inode(&self) -> (u64, u64) {
        self.inode
    }
}

pub fn snapshot_self() -> Option<ImageSnap> {
    // § e1s6kxps0.c@4mc0j
    let file = open_self_image()?;
    let meta = file.metadata().ok()?;
    Some(ImageSnap {
        inode: (meta.dev(), meta.ino()),
        _hold: file,
    })
}

/// Why an image reaches no hub.
pub(crate) const NO_IMAGE: &str = "Lade cannot read its own executable, so it reaches no hub";

/// 16 hex chars. Same image, same token. `cargo run` vs an install
/// get different sockets even when `CARGO_PKG_VERSION` matches. `None`: the
/// image cannot read its own file, and a shared fallback name would cross
/// binaries.
pub(crate) fn instance_token() -> Option<String> {
    // § hxw00jp6v.a@r83t6
    #[cfg(test)]
    if NO_IMAGE_FOR_TEST.with(std::cell::Cell::get) {
        return None;
    }
    static TOKEN: OnceLock<Option<String>> = OnceLock::new();
    TOKEN.get_or_init(image_token).clone()
}

#[cfg(test)]
thread_local! {
    pub(super) static NO_IMAGE_FOR_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn image_token() -> Option<String> {
    // § hxw00jp6v.a@67jxj
    let mut file = open_self_image()?;
    let meta = file.metadata().ok()?;
    let mut hasher = Sha256::new();
    hasher.update(meta.dev().to_le_bytes());
    hasher.update(meta.ino().to_le_bytes());
    hasher.update(meta.len().to_le_bytes());
    hasher.update(meta.mtime().to_le_bytes());
    let mut head = [0u8; 4096];
    let n = file.read(&mut head).ok()?;
    hasher.update(&head[..n]);
    let digest = hasher.finalize();
    let mut out = String::with_capacity(16);
    for b in digest.iter().take(8) {
        let _ = write!(&mut out, "{b:02x}");
    }
    Some(out)
}

/// The binary the hub serves: its own. Values cross the socket in
/// plaintext, so this is what keeps them in Lade.
pub struct Identity {
    pub inode: (u64, u64),
    /// macOS: the kernel's name for this code, when it is signed.
    #[cfg(target_os = "macos")]
    cdhash: Option<[u8; 20]>,
}

impl Identity {
    pub fn of_self(inode: (u64, u64)) -> Self {
        Self {
            inode,
            #[cfg(target_os = "macos")]
            cdhash: super::guard::own_cdhash(),
        }
    }
}

pub fn peer_allowed(stream: &impl AsRawFd, want: &Identity) -> bool {
    // § keyfb5k7w@x4ckf
    // § e1s6kxps0.a@pqbnz
    let Some(uid) = peer_uid(stream) else {
        return false;
    };
    if uid != current_uid() {
        return false;
    }
    // § end e1s6kxps0.a
    #[cfg(target_os = "macos")]
    if let Some(cdhash) = want.cdhash {
        // § e1s6kxps0.b@1kxja
        return super::guard::peer_cdhash(stream) == Some(cdhash);
    }
    let Some(pid) = peer_pid(stream) else {
        return false;
    };
    peer_exe_inode(pid) == Some(want.inode)
}

pub(crate) fn current_uid() -> u32 {
    unsafe { libc::getuid() }
}

pub(super) fn inode_of(path: &std::path::Path) -> Option<(u64, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.dev(), meta.ino()))
}

/// False once a reinstall put another file at this binary's path (Linux
/// reports the old one as deleted). New clients then hash another token.
pub(super) fn image_is_current(want: (u64, u64)) -> bool {
    // § cbz2he75h.b@gk3m7
    std::env::current_exe().ok().and_then(|exe| inode_of(&exe)) == Some(want)
}

#[cfg(target_os = "linux")]
fn open_self_image() -> Option<std::fs::File> {
    std::fs::File::open("/proc/self/exe").ok()
}

#[cfg(not(target_os = "linux"))]
fn open_self_image() -> Option<std::fs::File> {
    let exe = std::env::current_exe().ok()?;
    std::fs::File::open(exe).ok()
}

#[cfg(target_os = "linux")]
pub(crate) fn peer_uid(stream: &impl AsRawFd) -> Option<u32> {
    peer_ucred(stream).map(|c| c.uid)
}

#[cfg(target_os = "linux")]
fn peer_pid(stream: &impl AsRawFd) -> Option<u32> {
    peer_ucred(stream).map(|c| c.pid)
}

#[cfg(target_os = "linux")]
fn peer_ucred(stream: &impl AsRawFd) -> Option<Ucred> {
    let fd = stream.as_raw_fd();
    let mut cred = Ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<Ucred>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    (rc == 0).then_some(cred)
}

#[cfg(target_os = "linux")]
#[repr(C)]
struct Ucred {
    pid: u32,
    uid: u32,
    gid: u32,
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn peer_uid(stream: &impl AsRawFd) -> Option<u32> {
    let fd = stream.as_raw_fd();
    let mut uid = 0 as libc::uid_t;
    let mut gid = 0 as libc::gid_t;
    let rc = unsafe { libc::getpeereid(fd, &mut uid, &mut gid) };
    (rc == 0).then_some(uid)
}

#[cfg(target_os = "macos")]
fn peer_pid(stream: &impl AsRawFd) -> Option<u32> {
    const SOL_LOCAL: libc::c_int = 0;
    const LOCAL_PEERPID: libc::c_int = 0x002;
    let fd = stream.as_raw_fd();
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    let rc = unsafe {
        libc::getsockopt(
            fd,
            SOL_LOCAL,
            LOCAL_PEERPID,
            &mut pid as *mut _ as *mut libc::c_void,
            &mut len,
        )
    };
    (rc == 0 && pid > 0).then_some(pid as u32)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn peer_pid(_stream: &impl AsRawFd) -> Option<u32> {
    None
}

#[cfg(target_os = "linux")]
fn peer_exe_inode(pid: u32) -> Option<(u64, u64)> {
    // § e1s6kxps0.c@ckgpy
    inode_of(std::path::Path::new(&format!("/proc/{pid}/exe")))
}

#[cfg(target_os = "macos")]
fn peer_exe_inode(pid: u32) -> Option<(u64, u64)> {
    // § e1s6kxps0.c@az7wt
    let path = proc_pidpath(pid)?;
    inode_of(&path)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn peer_exe_inode(_pid: u32) -> Option<(u64, u64)> {
    None
}

#[cfg(target_os = "macos")]
fn proc_pidpath(pid: u32) -> Option<std::path::PathBuf> {
    let mut buf = [0u8; 4096];
    let n = unsafe {
        libc::proc_pidpath(
            pid as libc::c_int,
            buf.as_mut_ptr() as *mut libc::c_void,
            buf.len() as u32,
        )
    };
    if n <= 0 {
        return None;
    }
    let bytes = &buf[..n as usize];
    let text = std::str::from_utf8(bytes).ok()?;
    Some(std::path::PathBuf::from(text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_self_holds_an_inode() {
        let snap = snapshot_self().expect("self image");
        assert_ne!(snap.inode(), (0, 0));
    }

    /// A socketpair's peer is the process that made it, so the other
    /// binary connects to a listener itself.
    #[test]
    fn only_a_client_of_this_binary_is_allowed() {
        use std::os::unix::net::{UnixListener, UnixStream};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("s");
        let listener = UnixListener::bind(&path).unwrap();
        let me = Identity::of_self(snapshot_self().expect("self image").inode());
        let _client = UnixStream::connect(&path).unwrap();
        let (ours, _) = listener.accept().unwrap();
        assert!(peer_allowed(&ours, &me));
        let mut other = std::process::Command::new("nc")
            .arg("-U")
            .arg(&path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let (theirs, _) = listener.accept().unwrap();
        let allowed = peer_allowed(&theirs, &me);
        let _ = other.kill();
        let _ = other.wait();
        assert!(!allowed);
    }

    #[test]
    fn instance_token_is_stable_hex() {
        let a = instance_token().expect("own image");
        let b = instance_token().expect("own image");
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
