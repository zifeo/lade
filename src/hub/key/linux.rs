use super::{as_key, random_key};
use crate::hub::crypto::zero_key;

const ACCOUNT: &str = "lade-wrap-v2";

/// Session keyring, possessor-only. Create `@s` if this process has none.
pub(super) fn wrap_key() -> Result<[u8; 32], String> {
    use linux_keyutils::{KeyError, KeyRing, KeyRingIdentifier};

    let ring = KeyRing::from_special_id(KeyRingIdentifier::Session, true)
        .map_err(|error| format!("Session keyring unavailable ({error})."))?;
    match ring.search(ACCOUNT) {
        Ok(key) => {
            let mut buf = vec![0u8; 32];
            let n = key
                .read(&mut buf)
                .map_err(|error| format!("Session keyring read failed ({error})."))?;
            buf.truncate(n);
            as_key(&buf).ok_or_else(|| "Session keyring wrap key is not 32 bytes.".to_string())
        }
        Err(KeyError::KeyDoesNotExist) => {
            let mut minted =
                random_key().ok_or_else(|| "Could not mint a wrap key.".to_string())?;
            match ring.add_key(ACCOUNT, &minted) {
                Ok(_) => Ok(minted),
                Err(_) => match ring.search(ACCOUNT) {
                    Ok(key) => {
                        let mut buf = vec![0u8; 32];
                        let n = key.read(&mut buf).map_err(|error| {
                            zero_key(&mut minted);
                            format!("Session keyring read failed ({error}).")
                        })?;
                        buf.truncate(n);
                        zero_key(&mut minted);
                        as_key(&buf)
                            .ok_or_else(|| "Session keyring wrap key is not 32 bytes.".to_string())
                    }
                    Err(error) => {
                        zero_key(&mut minted);
                        Err(format!("Session keyring add failed ({error})."))
                    }
                },
            }
        }
        Err(error) => Err(format!("Session keyring search failed ({error}).")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_keyring_roundtrip() {
        let key = wrap_key().expect("session keyring wrap key");
        assert_eq!(key.len(), 32);
        let again = wrap_key().expect("session keyring read");
        assert_eq!(key, again);
    }
}
