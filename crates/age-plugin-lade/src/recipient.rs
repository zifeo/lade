use std::collections::HashSet;
use std::io;

use age_core::format::{FileKey, Stanza};
use age_plugin::Callbacks;
use age_plugin::recipient::{self, RecipientPluginV1};

use super::handler::{hydrate, lade_uri};
use super::parse::as_recipients;

enum EntryKind {
    Recipient,
    Identity,
}

struct Entry {
    index: usize,
    kind: EntryKind,
    uri: String,
}

type Natives = Result<Vec<Box<dyn age::Recipient + Send>>, Vec<recipient::Error>>;

pub struct LadeRecipient {
    entries: Vec<Entry>,
    natives: Option<Natives>,
}

impl LadeRecipient {
    pub fn new() -> Self {
        LadeRecipient {
            entries: Vec::new(),
            natives: None,
        }
    }
}

fn hydrate_recipients(entries: &[Entry]) -> Natives {
    let mut natives = Vec::new();
    let mut errors = Vec::new();
    for e in entries {
        match hydrate(&e.uri).and_then(|h| as_recipients(&h)) {
            Ok(mut r) => natives.append(&mut r),
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
    if errors.is_empty() {
        Ok(natives)
    } else {
        Err(errors)
    }
}

// The client checks labels before it hands over file keys, so a throwaway
// wrap is the only way to learn them. A post-quantum key (`postquantum`) must
// not share a file with a classic one, or the classic stanza undoes it.
fn shared_labels(
    natives: &[Box<dyn age::Recipient + Send>],
) -> Result<HashSet<String>, recipient::Error> {
    let probe = FileKey::new(Box::new([0; 16]));
    let mut shared: Option<HashSet<String>> = None;
    for native in natives {
        let (_, labels) = native
            .wrap_file_key(&probe)
            .map_err(|e| recipient::Error::Internal {
                message: e.to_string(),
            })?;
        match &shared {
            None => shared = Some(labels),
            Some(first) if *first == labels => {}
            Some(_) => {
                return Err(recipient::Error::Internal {
                    message: "post-quantum and classic keys cannot share a file".into(),
                });
            }
        }
    }
    Ok(shared.unwrap_or_default())
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
        let entries = &self.entries;
        let labels = match self
            .natives
            .get_or_insert_with(|| hydrate_recipients(entries))
        {
            Ok(natives) => shared_labels(natives),
            Err(_) => return HashSet::new(),
        };
        // labels() cannot fail, so wrap_file_keys reports the mismatch.
        labels.unwrap_or_else(|error| {
            self.natives = Some(Err(vec![error]));
            HashSet::new()
        })
    }

    fn wrap_file_keys(
        &mut self,
        file_keys: Vec<FileKey>,
        _callbacks: impl Callbacks<recipient::Error>,
    ) -> io::Result<Result<Vec<Vec<Stanza>>, Vec<recipient::Error>>> {
        let natives = match self.natives.take() {
            Some(natives) => natives,
            None => hydrate_recipients(&self.entries),
        };
        let natives = match natives {
            Ok(natives) => natives,
            Err(errors) => return Ok(Err(errors)),
        };
        let mut out = Vec::with_capacity(file_keys.len());
        for fk in &file_keys {
            let mut stanzas = Vec::new();
            for native in &natives {
                match native.wrap_file_key(fk) {
                    Ok((mut s, _)) => stanzas.append(&mut s),
                    Err(err) => {
                        return Ok(Err(vec![recipient::Error::Internal {
                            message: err.to_string(),
                        }]));
                    }
                }
            }
            out.push(stanzas);
        }
        Ok(Ok(out))
    }
}
