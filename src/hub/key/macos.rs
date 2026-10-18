use std::io::IsTerminal;

use super::{as_key, random_key};
use crate::hub::crypto::zero_key;
use crate::message_box::MessageBox;

const SERVICE: &str = "com.zifeo.lade.wrap";
const ACCOUNT: &str = "lade-wrap-v2";
const ERR_ITEM_NOT_FOUND: i32 = -25300;
const ERR_DUPLICATE_ITEM: i32 = -25299;
const ERR_INTERACTION_NOT_ALLOWED: i32 = -25308;
const ERR_AUTH_FAILED: i32 = -25293;

/// Login keychain. Allow the system ACL dialog so Always Allow can stick.
/// Serialize clients so concurrent calls cannot stack dialogs.
pub(super) fn wrap_key() -> Result<[u8; 32], String> {
    let _lock = wrap_key_lock()?;
    match copy_explaining_prompt(SERVICE, ACCOUNT) {
        Ok(key) => return Ok(key),
        Err(status) if status == ERR_ITEM_NOT_FOUND => {}
        Err(status) => return Err(sec_explain(status)),
    }
    let mut minted = random_key().ok_or_else(|| "Could not mint a wrap key.".to_string())?;
    let add = add(SERVICE, ACCOUNT, &minted, false);
    if add == 0 {
        return Ok(minted);
    }
    if add == ERR_DUPLICATE_ITEM {
        match copy(SERVICE, ACCOUNT, false) {
            Ok(key) => {
                zero_key(&mut minted);
                return Ok(key);
            }
            Err(status) => {
                zero_key(&mut minted);
                return Err(sec_explain(status));
            }
        }
    }
    if let Ok(key) = copy(SERVICE, ACCOUNT, false) {
        zero_key(&mut minted);
        return Ok(key);
    }
    zero_key(&mut minted);
    Err(sec_explain(add))
}

/// Reads once with the UI off. When macOS would show its access dialog, say
/// what the key is for before letting the dialog show.
fn copy_explaining_prompt(service: &str, account: &str) -> Result<[u8; 32], i32> {
    let silent = {
        let _no_ui = sec::DenyKeychainUi::enter();
        copy(service, account, false)
    };
    match silent {
        Err(status) if would_prompt(status) => {}
        other => return other,
    }
    if std::io::stderr().is_terminal() {
        MessageBox::new()
            .info()
            .line("macOS is about to ask for Keychain access for the Lade secret cache.")
            .paragraph(
                "Lade keeps one random key in its own login keychain item \
                 (com.zifeo.lade.wrap) and encrypts cached secrets with it. \
                 It does not read your saved passwords.",
            )
            .paragraph("Choose Always Allow so macOS stops asking for this Lade binary.")
            .print_stderr();
    }
    copy(service, account, false)
}

/// With the UI off, a locked keychain says so, but an item whose ACL lacks
/// this binary (any reinstall is a new identity) fails as a bad passphrase.
fn would_prompt(status: i32) -> bool {
    status == ERR_INTERACTION_NOT_ALLOWED || status == ERR_AUTH_FAILED
}

fn wrap_key_lock() -> Result<nix::fcntl::Flock<std::fs::File>, String> {
    use nix::fcntl::{Flock, FlockArg};

    let path = crate::cache::root().join("wrap-key.lock");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Could not create wrap key lock directory: {e}"))?;
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .map_err(|e| format!("Could not open wrap key lock: {e}"))?;
    Flock::lock(file, FlockArg::LockExclusive)
        .map_err(|(_, e)| format!("Could not lock wrap key: {e}"))
}

mod sec {
    use core_foundation::string::CFStringRef;

    #[link(name = "Security", kind = "framework")]
    unsafe extern "C" {
        pub fn SecKeychainGetUserInteractionAllowed(allowed: *mut u8) -> i32;
        pub fn SecKeychainSetUserInteractionAllowed(allowed: u8) -> i32;
        pub fn SecCopyErrorMessageString(
            status: i32,
            reserved: *const std::ffi::c_void,
        ) -> CFStringRef;
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

fn sec_explain(status: i32) -> String {
    format!(
        "Keychain refused the wrap key ({status} {}).",
        sec_copy_msg(status)
    )
}

fn sec_copy_msg(status: i32) -> String {
    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;
    unsafe {
        let raw = sec::SecCopyErrorMessageString(status, std::ptr::null());
        if raw.is_null() {
            return "unknown".to_string();
        }
        CFString::wrap_under_create_rule(raw).to_string()
    }
}

/// Login keychain unless `data_protection`. Tests disable UI with a guard.
fn copy(service: &str, account: &str, data_protection: bool) -> Result<[u8; 32], i32> {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::data::CFData;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::number::CFNumber;
    use core_foundation::string::CFString;
    use security_framework_sys::item::{
        kSecAttrAccount, kSecAttrService, kSecClass, kSecClassGenericPassword, kSecMatchLimit,
        kSecReturnData,
    };
    use security_framework_sys::keychain_item::SecItemCopyMatching;

    unsafe {
        let class = CFString::wrap_under_get_rule(kSecClass);
        let class_val = CFString::wrap_under_get_rule(kSecClassGenericPassword);
        let service_key = CFString::wrap_under_get_rule(kSecAttrService);
        let account_key = CFString::wrap_under_get_rule(kSecAttrAccount);
        let ret = CFString::wrap_under_get_rule(kSecReturnData);
        let limit = CFString::wrap_under_get_rule(kSecMatchLimit);
        let mut pairs = vec![
            (class.as_CFType(), class_val.as_CFType()),
            (service_key.as_CFType(), CFString::new(service).as_CFType()),
            (account_key.as_CFType(), CFString::new(account).as_CFType()),
            (ret.as_CFType(), CFBoolean::true_value().as_CFType()),
            (limit.as_CFType(), CFNumber::from(1i32).as_CFType()),
        ];
        let dp = CFString::wrap_under_get_rule(sec::kSecUseDataProtectionKeychain);
        if data_protection {
            pairs.push((dp.as_CFType(), CFBoolean::true_value().as_CFType()));
        }
        let query = CFDictionary::from_CFType_pairs(&pairs);
        let mut item = std::ptr::null();
        let status = SecItemCopyMatching(query.as_concrete_TypeRef(), &mut item);
        if status != 0 || item.is_null() {
            return Err(status);
        }
        let data = CFData::wrap_under_create_rule(item.cast());
        as_key(data.bytes()).ok_or(status)
    }
}

fn add(service: &str, account: &str, key: &[u8; 32], data_protection: bool) -> i32 {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::data::CFData;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    use security_framework_sys::item::{
        kSecAttrAccount, kSecAttrService, kSecClass, kSecClassGenericPassword, kSecValueData,
    };
    use security_framework_sys::keychain_item::SecItemAdd;

    unsafe {
        let class = CFString::wrap_under_get_rule(kSecClass);
        let class_val = CFString::wrap_under_get_rule(kSecClassGenericPassword);
        let service_key = CFString::wrap_under_get_rule(kSecAttrService);
        let account_key = CFString::wrap_under_get_rule(kSecAttrAccount);
        let value = CFString::wrap_under_get_rule(kSecValueData);
        let mut pairs = vec![
            (class.as_CFType(), class_val.as_CFType()),
            (service_key.as_CFType(), CFString::new(service).as_CFType()),
            (account_key.as_CFType(), CFString::new(account).as_CFType()),
            (value.as_CFType(), CFData::from_buffer(key).as_CFType()),
        ];
        let access = CFString::wrap_under_get_rule(sec::kSecAttrAccessible);
        let after =
            CFString::wrap_under_get_rule(sec::kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly);
        let dp = CFString::wrap_under_get_rule(sec::kSecUseDataProtectionKeychain);
        if data_protection {
            pairs.push((access.as_CFType(), after.as_CFType()));
            pairs.push((dp.as_CFType(), CFBoolean::true_value().as_CFType()));
        }
        let query = CFDictionary::from_CFType_pairs(&pairs);
        SecItemAdd(query.as_concrete_TypeRef(), std::ptr::null_mut())
    }
}

#[cfg(test)]
fn delete(service: &str, account: &str, data_protection: bool) -> i32 {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;
    use security_framework_sys::item::{
        kSecAttrAccount, kSecAttrService, kSecClass, kSecClassGenericPassword,
    };
    use security_framework_sys::keychain_item::SecItemDelete;

    unsafe {
        let class = CFString::wrap_under_get_rule(kSecClass);
        let class_val = CFString::wrap_under_get_rule(kSecClassGenericPassword);
        let service_key = CFString::wrap_under_get_rule(kSecAttrService);
        let account_key = CFString::wrap_under_get_rule(kSecAttrAccount);
        let mut pairs = vec![
            (class.as_CFType(), class_val.as_CFType()),
            (service_key.as_CFType(), CFString::new(service).as_CFType()),
            (account_key.as_CFType(), CFString::new(account).as_CFType()),
        ];
        let dp = CFString::wrap_under_get_rule(sec::kSecUseDataProtectionKeychain);
        if data_protection {
            pairs.push((dp.as_CFType(), CFBoolean::true_value().as_CFType()));
        }
        let query = CFDictionary::from_CFType_pairs(&pairs);
        SecItemDelete(query.as_concrete_TypeRef())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The login keychain follows `$HOME`, which other tests move through
    /// `temp_env`, and the UI switch is global to the process. Holding the
    /// `temp_env` lock pins HOME and keeps these tests from overlapping.
    fn on_keychain(test: impl FnOnce()) {
        temp_env::with_vars(Vec::<(&str, Option<&str>)>::new(), test);
    }

    #[test]
    fn login_keychain_roundtrip_without_ui() {
        on_keychain(|| {
            let _no_ui = sec::DenyKeychainUi::enter();
            let service = "com.zifeo.lade.wrap.test";
            let account = format!("lade-wrap-test-{}", std::process::id());
            let _ = delete(service, &account, false);
            let key = [7u8; 32];
            let added = add(service, &account, &key, false);
            assert_eq!(
                added,
                0,
                "login keychain add: {added} {}",
                sec_copy_msg(added)
            );
            let got = copy(service, &account, false);
            let removed = delete(service, &account, false);
            assert_eq!(got, Ok(key), "login keychain get after add");
            assert_eq!(removed, 0, "login keychain delete: {removed}");
        });
    }

    #[test]
    fn another_apps_item_is_refused_silently() {
        on_keychain(another_apps_item);
    }

    fn another_apps_item() {
        let service = "com.zifeo.lade.wrap.test";
        let account = format!("lade-wrap-foreign-{}", std::process::id());
        let security = |args: &[&str]| {
            std::process::Command::new("security")
                .args(args)
                .args(["-s", service, "-a", &account])
                .output()
                .unwrap()
        };
        let secret = "k".repeat(32);
        assert!(
            security(&["add-generic-password", "-w", &secret])
                .status
                .success()
        );
        let got = {
            let _no_ui = sec::DenyKeychainUi::enter();
            copy(service, &account, false)
        };
        security(&["delete-generic-password"]);
        match got {
            Err(status) => assert!(would_prompt(status), "{status} {}", sec_copy_msg(status)),
            Ok(_) => panic!("read another app's item without its ACL"),
        }
    }

    #[test]
    fn an_allowed_read_needs_no_explanation() {
        on_keychain(|| {
            let _no_ui = sec::DenyKeychainUi::enter();
            let service = "com.zifeo.lade.wrap.test";
            let account = format!("lade-wrap-explain-{}", std::process::id());
            let _ = delete(service, &account, false);
            assert_eq!(
                copy_explaining_prompt(service, &account),
                Err(ERR_ITEM_NOT_FOUND)
            );
            let key = [5u8; 32];
            assert_eq!(add(service, &account, &key, false), 0);
            let got = copy_explaining_prompt(service, &account);
            let _ = delete(service, &account, false);
            assert_eq!(got, Ok(key));
        });
    }

    #[test]
    fn data_protection_keychain_is_not_for_a_cli() {
        on_keychain(|| {
            let _no_ui = sec::DenyKeychainUi::enter();
            let service = "com.zifeo.lade.wrap.test";
            let account = format!("lade-wrap-dp-{}", std::process::id());
            let key = [9u8; 32];
            let added = add(service, &account, &key, true);
            let _ = delete(service, &account, true);
            assert_ne!(
                added, 0,
                "data-protection add succeeded; a CLI is not supposed to use that store"
            );
        });
    }
}
