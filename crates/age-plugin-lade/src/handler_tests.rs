use super::*;
use age_core::plugin::{Error, Result as PluginResult};
use age_core::secrecy::{ExposeSecret, SecretString};
use age_plugin::Callbacks;
use age_plugin::recipient;

struct Noop<E>(std::marker::PhantomData<E>);

impl<E> Noop<E> {
    fn new() -> Self {
        Self(std::marker::PhantomData)
    }
}

impl<E> Callbacks<E> for Noop<E> {
    fn message(&mut self, _: &str) -> PluginResult<()> {
        Ok(Ok(()))
    }

    fn confirm(&mut self, _: &str, _: &str, _: Option<&str>) -> PluginResult<bool> {
        Ok(Ok(true))
    }

    fn request_public(&mut self, _: &str) -> PluginResult<String> {
        Ok(Err(Error::Fail))
    }

    fn request_secret(&mut self, _: &str) -> PluginResult<SecretString> {
        Ok(Err(Error::Fail))
    }

    fn error(&mut self, _: E) -> PluginResult<()> {
        Ok(Ok(()))
    }
}

fn file_uri(dir: &std::path::Path, secret: &str) -> String {
    let path = dir.join("key.json");
    std::fs::write(&path, format!(r#"{{"key":"{secret}"}}"#)).unwrap();
    format!("file://{}?query=.key", path.display())
}

fn workspace_lade() -> std::path::PathBuf {
    let exe = std::env::current_exe().expect("current_exe");
    if let Some(dir) = exe.parent() {
        if let Some(parent) = dir.parent() {
            let candidate = parent.join("lade");
            if candidate.is_file() {
                return candidate;
            }
        }
        let sibling = dir.join("lade");
        if sibling.is_file() {
            return sibling;
        }
    }
    panic!(
        "lade binary not found next to {}. Run cargo test --workspace --locked.",
        exe.display()
    );
}

#[test]
fn wrap_and_unwrap_via_file_uri() {
    let dir = tempfile::tempdir().unwrap();
    let id = age::x25519::Identity::generate();
    let secret = id.to_string().expose_secret().to_string();
    let uri = file_uri(dir.path(), &secret);
    let events = dir.path().join("events.db");
    let lade = workspace_lade();
    temp_env::with_vars(
        [
            ("LADE_EVENTS_PATH", Some(events.to_str().unwrap())),
            ("LADE_BIN", Some(lade.to_str().unwrap())),
        ],
        || {
            let mut rec = LadeRecipient {
                entries: Vec::new(),
            };
            rec.add_recipient(0, PLUGIN_NAME, uri.as_bytes())
                .unwrap_or_else(|_| panic!("add recipient"));
            let want = FileKey::new(Box::new([9u8; 16]));
            let stanzas =
                match rec.wrap_file_keys(vec![FileKey::new(Box::new([9u8; 16]))], Noop::new()) {
                    Ok(Ok(s)) => s,
                    Ok(Err(_)) => panic!("wrap failed"),
                    Err(e) => panic!("wrap io: {e}"),
                };
            assert_eq!(stanzas[0][0].tag.to_ascii_lowercase(), "x25519");

            let mut ident = LadeIdentity {
                identities: Vec::new(),
            };
            ident
                .add_identity(0, PLUGIN_NAME, uri.as_bytes())
                .unwrap_or_else(|_| panic!("add identity"));
            let out = ident
                .unwrap_file_keys(stanzas, Noop::new())
                .unwrap_or_else(|e| panic!("unwrap io: {e}"));
            let got = match out.get(&0) {
                Some(Ok(fk)) => fk,
                _ => panic!("missing unwrapped file key"),
            };
            assert_eq!(got.expose_secret(), want.expose_secret());
        },
    );
}

#[test]
fn missing_lade_is_a_hydrate_error() {
    temp_env::with_var("LADE_BIN", Some("/no/such/lade"), || {
        let mut rec = LadeRecipient {
            entries: Vec::new(),
        };
        rec.add_recipient(0, PLUGIN_NAME, b"file:///tmp/x?query=.k")
            .unwrap_or_else(|_| panic!("add recipient"));
        match rec.wrap_file_keys(vec![FileKey::new(Box::new([1u8; 16]))], Noop::new()) {
            Ok(Err(errs)) => {
                let msg: String = errs
                    .iter()
                    .map(|e| match e {
                        recipient::Error::Recipient { message, .. }
                        | recipient::Error::Identity { message, .. }
                        | recipient::Error::Internal { message } => message.as_str(),
                    })
                    .collect::<Vec<_>>()
                    .join(" ");
                assert!(msg.contains("LADE_BIN") || msg.contains("lade"), "{msg}");
            }
            Ok(Ok(_)) => panic!("expected hydrate error"),
            Err(e) => panic!("wrap io: {e}"),
        }
    });
}

#[test]
fn unknown_plugin_name_is_rejected() {
    let mut rec = LadeRecipient {
        entries: Vec::new(),
    };
    assert!(rec.add_recipient(0, "other", b"op://v/i/f").is_err());
    let mut ident = LadeIdentity {
        identities: Vec::new(),
    };
    assert!(ident.add_identity(0, "other", b"op://v/i/f").is_err());
}
