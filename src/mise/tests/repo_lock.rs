use super::*;
use crate::mise::lock;
use crate::mise::spec;
use crate::mise::store;
use std::path::PathBuf;

#[test]
fn root_lade_lock_covers_every_yaml_pin() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let lock_path = root.join("lade.lock");
    assert!(
        lock_path.is_file(),
        "lade.lock must exist next to lade.yml: {}",
        lock_path.display()
    );
    let config = LadeFile::build(root).unwrap();
    let pins = config.pins(&None);
    assert!(
        !pins.is_empty(),
        "root lade.yml must declare at least one mise pin"
    );
    for (key, uri) in pins {
        let parsed = spec::parse(&uri).expect(key.as_str());
        let names = store::tool_names(&parsed, &key, None);
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let slot = lock::slot_for(&lock_path, &name_refs).or_else(|| {
            lock::read_tools(&lock_path).and_then(|slots| {
                slots
                    .into_iter()
                    .find(|slot| slot.backend.as_deref() == Some(parsed.backend_id().as_str()))
            })
        });
        assert!(
            slot.is_some(),
            "lade.lock is missing {key} ({uri}, backend {})",
            parsed.backend_id()
        );
        let slot = slot.expect(key.as_str());
        assert!(
            !slot.version.is_empty(),
            "lade.lock version for {key} is empty"
        );
        assert!(
            lock::agrees(&slot, &parsed.version, &parsed.backend_id()),
            "lade.lock {key} {} does not satisfy {}",
            slot.version,
            parsed.version
        );
    }
}
