use super::crypto::zero_key;

const SERVICE: &str = "com.zifeo.lade.wrap";
const ACCOUNT: &str = "lade-wrap-v2";

pub fn wrap_key() -> Option<[u8; 32]> {
    match std::env::var("LADE_WRAP_KEY") {
        Ok(raw) => decode_hex_key(&raw),
        Err(_) => platform_wrap_key(),
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
fn platform_wrap_key() -> Option<[u8; 32]> {
    if std::env::var_os("CARGO_BIN_EXE_lade").is_some() {
        return None;
    }
    macos_wrap_key()
}

#[cfg(target_os = "linux")]
fn platform_wrap_key() -> Option<[u8; 32]> {
    linux_wrap_key()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn platform_wrap_key() -> Option<[u8; 32]> {
    None
}

fn random_key() -> Option<[u8; 32]> {
    use chacha20poly1305::aead::OsRng;
    use chacha20poly1305::aead::rand_core::RngCore;
    let mut key = [0u8; 32];
    OsRng.fill_bytes(&mut key);
    Some(key)
}

#[cfg(target_os = "macos")]
fn macos_wrap_key() -> Option<[u8; 32]> {
    let _no_ui = macos_sec::DenyKeychainUi::enter();
    if let Some(key) = macos_get() {
        return Some(key);
    }
    let mut minted = random_key()?;
    if macos_add(&minted) {
        return Some(minted);
    }
    let existing = macos_get();
    zero_key(&mut minted);
    existing
}

#[cfg(target_os = "macos")]
mod macos_sec {
    use core_foundation::string::CFStringRef;

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        pub fn SecKeychainGetUserInteractionAllowed(allowed: *mut u8) -> i32;
        pub fn SecKeychainSetUserInteractionAllowed(allowed: u8) -> i32;
        pub static kSecAttrAccessible: CFStringRef;
        pub static kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly: CFStringRef;
        pub static kSecUseDataProtectionKeychain: CFStringRef;
    }

    /// Login-keychain ACL prompts ignore `kSecUseAuthenticationUISkip`.
    pub struct DenyKeychainUi {
        prev: u8,
    }

    impl DenyKeychainUi {
        pub fn enter() -> Self {
            let mut prev = 1u8;
            unsafe {
                let _ = SecKeychainGetUserInteractionAllowed(&mut prev);
                let _ = SecKeychainSetUserInteractionAllowed(0);
            }
            Self { prev }
        }
    }

    impl Drop for DenyKeychainUi {
        fn drop(&mut self) {
            unsafe {
                let _ = SecKeychainSetUserInteractionAllowed(self.prev);
            }
        }
    }
}

/// Silent SecItem. Skip UI. If a dialog would be needed, skip the hub.
#[cfg(target_os = "macos")]
fn macos_get() -> Option<[u8; 32]> {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::data::CFData;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use security_framework_sys::item::{
        kSecAttrAccount, kSecAttrService, kSecClass, kSecClassGenericPassword, kSecMatchLimit,
        kSecReturnData, kSecUseAuthenticationUI, kSecUseAuthenticationUISkip,
    };
    use security_framework_sys::keychain_item::SecItemCopyMatching;

    unsafe {
        let class = CFString::wrap_under_get_rule(kSecClass);
        let class_val = CFString::wrap_under_get_rule(kSecClassGenericPassword);
        let service = CFString::wrap_under_get_rule(kSecAttrService);
        let account = CFString::wrap_under_get_rule(kSecAttrAccount);
        let ret = CFString::wrap_under_get_rule(kSecReturnData);
        let limit = CFString::wrap_under_get_rule(kSecMatchLimit);
        let ui = CFString::wrap_under_get_rule(kSecUseAuthenticationUI);
        let skip = CFString::wrap_under_get_rule(kSecUseAuthenticationUISkip);
        let dp = CFString::wrap_under_get_rule(macos_sec::kSecUseDataProtectionKeychain);
        let query = CFDictionary::from_CFType_pairs(&[
            (class.as_CFType(), class_val.as_CFType()),
            (service.as_CFType(), CFString::new(SERVICE).as_CFType()),
            (account.as_CFType(), CFString::new(ACCOUNT).as_CFType()),
            (ret.as_CFType(), CFBoolean::true_value().as_CFType()),
            (limit.as_CFType(), CFNumber::from(1i32).as_CFType()),
            (ui.as_CFType(), skip.as_CFType()),
            (dp.as_CFType(), CFBoolean::true_value().as_CFType()),
        ]);
        let mut item = std::ptr::null();
        let status = SecItemCopyMatching(query.as_concrete_TypeRef(), &mut item);
        if status != 0 || item.is_null() {
            return None;
        }
        let data = CFData::wrap_under_create_rule(item.cast());
        as_key(data.bytes())
    }
}

#[cfg(target_os = "macos")]
fn macos_add(key: &[u8; 32]) -> bool {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::data::CFData;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    use security_framework_sys::item::{
        kSecAttrAccount, kSecAttrService, kSecClass, kSecClassGenericPassword,
        kSecUseAuthenticationUI, kSecUseAuthenticationUISkip, kSecValueData,
    };
    use security_framework_sys::keychain_item::SecItemAdd;

    unsafe {
        let class = CFString::wrap_under_get_rule(kSecClass);
        let class_val = CFString::wrap_under_get_rule(kSecClassGenericPassword);
        let service = CFString::wrap_under_get_rule(kSecAttrService);
        let account = CFString::wrap_under_get_rule(kSecAttrAccount);
        let value = CFString::wrap_under_get_rule(kSecValueData);
        let access = CFString::wrap_under_get_rule(macos_sec::kSecAttrAccessible);
        let after = CFString::wrap_under_get_rule(
            macos_sec::kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
        );
        let ui = CFString::wrap_under_get_rule(kSecUseAuthenticationUI);
        let skip = CFString::wrap_under_get_rule(kSecUseAuthenticationUISkip);
        let dp = CFString::wrap_under_get_rule(macos_sec::kSecUseDataProtectionKeychain);
        let query = CFDictionary::from_CFType_pairs(&[
            (class.as_CFType(), class_val.as_CFType()),
            (service.as_CFType(), CFString::new(SERVICE).as_CFType()),
            (account.as_CFType(), CFString::new(ACCOUNT).as_CFType()),
            (value.as_CFType(), CFData::from_buffer(key).as_CFType()),
            (access.as_CFType(), after.as_CFType()),
            (ui.as_CFType(), skip.as_CFType()),
            (dp.as_CFType(), CFBoolean::true_value().as_CFType()),
        ]);
        let status = SecItemAdd(query.as_concrete_TypeRef(), std::ptr::null_mut());
        status == 0
    }
}

#[cfg(target_os = "linux")]
fn linux_wrap_key() -> Option<[u8; 32]> {
    use linux_keyutils::{KeyError, KeyRing, KeyRingIdentifier};

    let ring = KeyRing::from_special_id(KeyRingIdentifier::Session, false).ok()?;
    match ring.search(ACCOUNT) {
        Ok(key) => {
            let mut buf = vec![0u8; 32];
            let n = key.read(&mut buf).ok()?;
            buf.truncate(n);
            as_key(&buf)
        }
        Err(KeyError::KeyDoesNotExist) => {
            let mut minted = random_key()?;
            match ring.add_key(ACCOUNT, &minted) {
                Ok(_) => Some(minted),
                Err(_) => match ring.search(ACCOUNT) {
                    Ok(key) => {
                        let mut buf = vec![0u8; 32];
                        let n = key.read(&mut buf).ok()?;
                        buf.truncate(n);
                        zero_key(&mut minted);
                        as_key(&buf)
                    }
                    Err(_) => {
                        zero_key(&mut minted);
                        None
                    }
                },
            }
        }
        Err(_) => None,
    }
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
        });
    }
}
