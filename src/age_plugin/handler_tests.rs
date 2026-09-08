use super::*;
use age_core::plugin::{Error, Result as PluginResult};
use age_core::secrecy::{ExposeSecret, SecretString};
use age_plugin::Callbacks;

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

#[test]
fn wrap_and_unwrap_via_file_uri() {
    let dir = tempfile::tempdir().unwrap();
    let id = age::x25519::Identity::generate();
    let secret = id.to_string().expose_secret().to_string();
    let uri = file_uri(dir.path(), &secret);
    let events = dir.path().join("events.db");
    temp_env::with_var("LADE_EVENTS_PATH", Some(events.to_str().unwrap()), || {
        let mut rec = LadeRecipient {
            entries: Vec::new(),
        };
        rec.add_recipient(0, PLUGIN_NAME, uri.as_bytes())
            .unwrap_or_else(|_| panic!("add recipient"));
        let want = FileKey::new(Box::new([9u8; 16]));
        let stanzas = match rec.wrap_file_keys(vec![FileKey::new(Box::new([9u8; 16]))], Noop::new())
        {
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
