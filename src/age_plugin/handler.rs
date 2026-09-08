use std::collections::{HashMap, HashSet};
use std::io;
use std::path::PathBuf;
use std::sync::OnceLock;

use age_core::format::{FileKey, Stanza};
use age_plugin::identity::{self, IdentityPluginV1};
use age_plugin::recipient::{self, RecipientPluginV1};
use age_plugin::{Callbacks, PluginHandler};
use tokio::runtime::Runtime;

use super::parse::{as_identities, as_recipients};
use crate::args::Command;
use crate::audience;
use crate::eval;

pub const PLUGIN_NAME: &str = "lade";

fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("age-plugin-lade tokio runtime")
    })
}

fn hydrate(uri: &str) -> Result<String, String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let stdin = std::io::IsTerminal::is_terminal(&std::io::stdin());
    let stderr = std::io::IsTerminal::is_terminal(&std::io::stderr());
    let d = audience::detect(
        &Command::Eval {
            uri: uri.to_string(),
        },
        false,
        stdin,
        stderr,
    )
    .expect("eval detect");
    runtime()
        .block_on(eval::resolve_uri(
            uri.to_string(),
            &cwd,
            super::link::PLUGIN_BIN,
            d.via,
            d.audience,
        ))
        .map_err(|e| e.to_string())
}

fn lade_uri(name: &str, bytes: &[u8]) -> Result<String, String> {
    if name != PLUGIN_NAME {
        return Err(format!("unknown plugin {name}"));
    }
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| "payload is not UTF-8".into())
}

pub struct Handler;

impl PluginHandler for Handler {
    type RecipientV1 = LadeRecipient;
    type IdentityV1 = LadeIdentity;

    fn recipient_v1(self) -> io::Result<Self::RecipientV1> {
        Ok(LadeRecipient {
            entries: Vec::new(),
        })
    }

    fn identity_v1(self) -> io::Result<Self::IdentityV1> {
        Ok(LadeIdentity {
            identities: Vec::new(),
        })
    }
}

enum EntryKind {
    Recipient,
    Identity,
}

struct Entry {
    index: usize,
    kind: EntryKind,
    uri: String,
}

pub struct LadeRecipient {
    entries: Vec<Entry>,
}

impl RecipientPluginV1 for LadeRecipient {
    fn add_recipient(
        &mut self,
        index: usize,
        plugin_name: &str,
        bytes: &[u8],
    ) -> Result<(), recipient::Error> {
        let uri = lade_uri(plugin_name, bytes)
            .map_err(|message| recipient::Error::Recipient { index, message })?;
        self.entries.push(Entry {
            index,
            kind: EntryKind::Recipient,
            uri,
        });
        Ok(())
    }

    fn add_identity(
        &mut self,
        index: usize,
        plugin_name: &str,
        bytes: &[u8],
    ) -> Result<(), recipient::Error> {
        let uri = lade_uri(plugin_name, bytes)
            .map_err(|message| recipient::Error::Identity { index, message })?;
        self.entries.push(Entry {
            index,
            kind: EntryKind::Identity,
            uri,
        });
        Ok(())
    }

    fn labels(&mut self) -> HashSet<String> {
        HashSet::new()
    }

    fn wrap_file_keys(
        &mut self,
        file_keys: Vec<FileKey>,
        _callbacks: impl Callbacks<recipient::Error>,
    ) -> io::Result<Result<Vec<Vec<Stanza>>, Vec<recipient::Error>>> {
        let mut natives = Vec::new();
        let mut errors = Vec::new();
        for e in &self.entries {
            match hydrate(&e.uri).and_then(|h| as_recipients(&h)) {
                Ok(r) => natives.push(r),
                Err(message) => errors.push(match e.kind {
                    EntryKind::Recipient => recipient::Error::Recipient {
                        index: e.index,
                        message,
                    },
                    EntryKind::Identity => recipient::Error::Identity {
                        index: e.index,
                        message,
                    },
                }),
            }
        }
        if !errors.is_empty() {
            return Ok(Err(errors));
        }
        let mut out = Vec::with_capacity(file_keys.len());
        for fk in &file_keys {
            let mut stanzas = Vec::new();
            for group in &natives {
                for native in group {
                    match native.wrap_file_key(fk) {
                        Ok((mut s, _)) => stanzas.append(&mut s),
                        Err(err) => {
                            return Ok(Err(vec![recipient::Error::Internal {
                                message: err.to_string(),
                            }]));
                        }
                    }
                }
            }
            out.push(stanzas);
        }
        Ok(Ok(out))
    }
}

pub struct LadeIdentity {
    identities: Vec<(usize, String)>,
}

impl IdentityPluginV1 for LadeIdentity {
    fn add_identity(
        &mut self,
        index: usize,
        plugin_name: &str,
        bytes: &[u8],
    ) -> Result<(), identity::Error> {
        let uri = lade_uri(plugin_name, bytes)
            .map_err(|message| identity::Error::Identity { index, message })?;
        self.identities.push((index, uri));
        Ok(())
    }

    fn unwrap_file_keys(
        &mut self,
        files: Vec<Vec<Stanza>>,
        _callbacks: impl Callbacks<identity::Error>,
    ) -> io::Result<HashMap<usize, Result<FileKey, Vec<identity::Error>>>> {
        let mut natives = Vec::new();
        let mut errors = Vec::new();
        for (index, uri) in &self.identities {
            match hydrate(uri).and_then(|h| as_identities(&h)) {
                Ok(id) => natives.push(id),
                Err(message) => errors.push(identity::Error::Identity {
                    index: *index,
                    message,
                }),
            }
        }
        if !errors.is_empty() {
            let mut map = HashMap::new();
            if !files.is_empty() {
                map.insert(0, Err(errors));
            }
            return Ok(map);
        }

        let mut out = HashMap::new();
        for (file_index, stanzas) in files.into_iter().enumerate() {
            let mut file_errors = Vec::new();
            let mut found = None;
            for group in &natives {
                for native in group {
                    match native.unwrap_stanzas(&stanzas) {
                        Some(Ok(fk)) => {
                            found = Some(fk);
                            break;
                        }
                        Some(Err(err)) => file_errors.push(identity::Error::Stanza {
                            file_index,
                            stanza_index: 0,
                            message: err.to_string(),
                        }),
                        None => {}
                    }
                }
                if found.is_some() {
                    break;
                }
            }
            if let Some(fk) = found {
                out.insert(file_index, Ok(fk));
            } else if !file_errors.is_empty() {
                out.insert(file_index, Err(file_errors));
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
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
}
