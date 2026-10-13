use super::crypto::zero_key;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

pub fn wrap_key() -> Option<[u8; 32]> {
    wrap_lookup().ok()
}

/// Why `wrap_key` is missing. None when a key is ready.
pub fn wrap_gap() -> Option<String> {
    wrap_lookup().err()
}

fn wrap_lookup() -> Result<[u8; 32], String> {
    match std::env::var("LADE_WRAP_KEY") {
        Ok(raw) => {
            decode_hex_key(&raw).ok_or_else(|| "LADE_WRAP_KEY is not 64 hex chars.".to_string())
        }
        Err(_) => match platform_wrap_key() {
            Ok(key) => Ok(key),
            Err(reason) => file_wrap_key().ok_or(reason),
        },
    }
}

fn file_wrap_key() -> Option<[u8; 32]> {
    if std::env::var_os("CARGO_BIN_EXE_lade").is_some() {
        return None;
    }
    let path = crate::cache::root().join("wrap-key");
    if let Ok(bytes) = std::fs::read(&path) {
        return as_key(&bytes);
    }
    let mut minted = random_key()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    match opts.open(&path) {
        Ok(mut file) => {
            use std::io::Write;
            if file.write_all(&minted).is_err() {
                zero_key(&mut minted);
                return None;
            }
            Some(minted)
        }
        Err(_) => {
            zero_key(&mut minted);
            std::fs::read(&path).ok().and_then(|bytes| as_key(&bytes))
        }
    }
}

fn decode_hex_key(raw: &str) -> Option<[u8; 32]> {
    let raw = raw.trim();
    if raw.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

#[cfg(target_os = "macos")]
fn platform_wrap_key() -> Result<[u8; 32], String> {
    if std::env::var_os("CARGO_BIN_EXE_lade").is_some() {
        return Err("Keychain skipped under cargo test.".to_string());
    }
    macos::wrap_key()
}

#[cfg(target_os = "linux")]
fn platform_wrap_key() -> Result<[u8; 32], String> {
    linux::wrap_key()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_wrap_key() -> Result<[u8; 32], String> {
    Err("No platform wrap key on this OS.".to_string())
}

fn random_key() -> Option<[u8; 32]> {
    use chacha20poly1305::aead::OsRng;
    use chacha20poly1305::aead::rand_core::RngCore;
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    Some(key)
}

fn as_key(bytes: &[u8]) -> Option<[u8; 32]> {
    <[u8; 32]>::try_from(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_key_parses() {
        let hex = "ab".repeat(32);
        assert!(decode_hex_key(&hex).is_some());
        assert!(decode_hex_key("zz").is_none());
        assert!(decode_hex_key(&"ab".repeat(31)).is_none());
    }

    #[test]
    fn bad_wrap_key_skips_platform() {
        temp_env::with_var("LADE_WRAP_KEY", Some("zz"), || {
            assert!(wrap_key().is_none());
            assert_eq!(
                wrap_gap().as_deref(),
                Some("LADE_WRAP_KEY is not 64 hex chars.")
            );
        });
    }

    #[test]
    fn file_wrap_key_roundtrips_without_platform() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().to_string_lossy().into_owned();
        temp_env::with_vars(
            [
                ("LADE_WRAP_KEY", None),
                ("LADE_CACHE_DIR", Some(cache.as_str())),
                ("CARGO_BIN_EXE_lade", None),
            ],
            || {
                let first = file_wrap_key().expect("mint file wrap key");
                let second = file_wrap_key().expect("read file wrap key");
                assert_eq!(first, second);
                let path = std::path::Path::new(&cache).join("wrap-key");
                assert_eq!(std::fs::read(&path).unwrap().len(), 32);
            },
        );
    }
}
