use std::collections::{HashMap, HashSet};
use std::io;
use std::process::Stdio;

use age_core::format::{FileKey, Stanza};
use age_plugin::identity::{self, IdentityPluginV1};
use age_plugin::recipient::{self, RecipientPluginV1};
use age_plugin::{Callbacks, PluginHandler};

use super::link::{PLUGIN_BIN, find_lade};
use super::parse::{as_identities, as_recipients};

pub const PLUGIN_NAME: &str = "lade";

fn strip_println_newline(s: &str) -> &str {
    s.strip_suffix("\r\n")
        .or_else(|| s.strip_suffix('\n'))
        .unwrap_or(s)
}

fn hydrate(uri: &str) -> Result<String, String> {
    let bin = find_lade()?;
    let output = std::process::Command::new(&bin)
        .arg("eval")
        .arg("--access-command")
        .arg(PLUGIN_BIN)
        .arg("--")
        .arg(uri)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("failed to run lade eval: {e}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        if stderr.is_empty() {
            return Err(format!("lade eval failed ({})", output.status));
        }
        return Err(stderr.to_string());
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|_| "lade eval output is not UTF-8".to_string())?;
    Ok(strip_println_newline(&stdout).to_string())
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
#[path = "handler_tests.rs"]
mod tests;
