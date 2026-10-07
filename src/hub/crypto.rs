use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use zeroize::Zeroize;

const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;

pub fn aad(cwd: &str, path: &str, rule: &str, when: &str, walk: &[u8; 32], user: &str) -> Vec<u8> {
    let mut out = Vec::new();
    push_len_str(&mut out, cwd);
    push_len_str(&mut out, path);
    push_len_str(&mut out, rule);
    push_len_str(&mut out, when);
    out.extend_from_slice(walk);
    push_len_str(&mut out, user);
    out
}

fn push_len_str(out: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    out.extend_from_slice(&(bytes.len() as u64).to_be_bytes());
    out.extend_from_slice(bytes);
}

pub fn seal(key: &[u8; 32], aad: &[u8], plaintext: &[u8]) -> Option<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    {
        use chacha20poly1305::aead::rand_core::RngCore;
        chacha20poly1305::aead::OsRng.fill_bytes(&mut nonce_bytes);
    }
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ct = cipher
        .encrypt(
            nonce,
            Payload {
                msg: plaintext,
                aad,
            },
        )
        .ok()?;
    let mut out = Vec::with_capacity(NONCE_LEN + ct.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ct);
    Some(out)
}

pub fn open(key: &[u8; 32], aad: &[u8], blob: &[u8]) -> Option<Vec<u8>> {
    if blob.len() < NONCE_LEN + TAG_LEN {
        return None;
    }
    let (nonce_bytes, ct) = blob.split_at(NONCE_LEN);
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    cipher
        .decrypt(Nonce::from_slice(nonce_bytes), Payload { msg: ct, aad })
        .ok()
}

pub fn zero_key(key: &mut [u8; 32]) {
    key.zeroize();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aad_length_prefix_does_not_glue() {
        let walk = [7u8; 32];
        let a = aad("/a", "bc", "r", "always", &walk, "");
        let b = aad("/ab", "c", "r", "always", &walk, "");
        assert_ne!(a, b);
    }

    #[test]
    fn seal_open_roundtrip() {
        let key = [9u8; 32];
        let good = aad(
            "/cwd",
            "/x/lade.yaml",
            "terraform .*",
            "always",
            &[1u8; 32],
            "alice",
        );
        let blob = seal(&key, &good, b"AKIASECRET").unwrap();
        assert_eq!(open(&key, &good, &blob).unwrap(), b"AKIASECRET");
        let other = aad(
            "/cwd",
            "/x/lade.yaml",
            "terraform .*",
            "human",
            &[1u8; 32],
            "alice",
        );
        assert!(open(&key, &other, &blob).is_none());
    }
}
