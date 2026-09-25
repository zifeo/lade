use std::path::Path;

use toml_edit::{DocumentMut, Item, Key, Table, value};

/// Upsert `[tools]` entries. Other tables and comments stay.
pub fn upsert_tools(path: &Path, entries: &[(String, String)]) -> Result<(), String> {
    if path.exists() && path.is_dir() {
        return Err(format!("{} is a directory", path.display()));
    }

    let mut doc = if path.is_file() {
        let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        raw.parse::<DocumentMut>()
            .map_err(|e| format!("invalid TOML at {}: {e}", path.display()))?
    } else if path.exists() {
        return Err(format!("{} is not a file", path.display()));
    } else {
        DocumentMut::new()
    };

    apply_entries(&mut doc, entries)?;
    write_atomic(path, &doc.to_string())
}

pub fn remove_tool_keys(path: &Path, keys: &[String]) -> Result<(), String> {
    if keys.is_empty() || !path.is_file() {
        return Ok(());
    }
    let raw = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let mut doc = raw
        .parse::<DocumentMut>()
        .map_err(|e| format!("invalid TOML at {}: {e}", path.display()))?;
    let Some(tools) = doc.get_mut("tools").and_then(Item::as_table_mut) else {
        return Ok(());
    };
    let mut changed = false;
    for key in keys {
        if tools.remove(key).is_some() {
            changed = true;
        }
    }
    if !changed {
        return Ok(());
    }
    write_atomic(path, &doc.to_string())
}

pub fn tool_version(path: &Path, key: &str) -> Option<String> {
    let raw = std::fs::read_to_string(path).ok()?;
    let doc = raw.parse::<DocumentMut>().ok()?;
    let tools = doc.get("tools")?;
    if let Some(item) = tools.get(key) {
        return version_from_item(item);
    }
    let want = short_tool_name(key);
    let table = tools.as_table()?;
    table.iter().find_map(|(stored, item)| {
        if short_tool_name(stored) == want {
            version_from_item(item)
        } else {
            None
        }
    })
}

fn apply_entries(doc: &mut DocumentMut, entries: &[(String, String)]) -> Result<(), String> {
    if doc.get("tools").is_none() {
        doc["tools"] = toml_edit::table();
    }
    let tools = doc["tools"]
        .as_table_mut()
        .ok_or_else(|| "[tools] is not a table".to_string())?;
    for (key, version) in entries {
        upsert_one(tools, key, version);
    }
    Ok(())
}

fn upsert_one(tools: &mut Table, key: &str, version: &str) {
    if tools.get(key).is_none() {
        insert_tool_string(tools, key, version);
        return;
    }
    let item = tools.get_mut(key).expect("key present after get");
    if item.is_str() {
        if item.as_str() != Some(version) {
            *item = value(version);
        }
        return;
    }
    if item.is_table() || item.is_inline_table() {
        let current = item
            .get("version")
            .and_then(Item::as_str)
            .map(str::to_string);
        if current.as_deref() != Some(version) {
            item["version"] = value(version);
        }
        return;
    }
    *item = value(version);
}

fn insert_tool_string(tools: &mut Table, key: &str, version: &str) {
    let formatted = tool_key(key);
    tools.insert_formatted(&formatted, value(version));
}

fn tool_key(key: &str) -> Key {
    if !needs_quotes(key) {
        return Key::new(key);
    }
    let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
    let mut parsed = Key::parse(&format!("\"{escaped}\"")).unwrap_or_else(|_| vec![Key::new(key)]);
    if parsed.len() == 1 {
        parsed.remove(0)
    } else {
        Key::new(key)
    }
}

fn needs_quotes(key: &str) -> bool {
    key.chars()
        .any(|ch| !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-')
}

fn version_from_item(item: &Item) -> Option<String> {
    if let Some(raw) = item.as_str() {
        return Some(raw.to_string());
    }
    item.get("version")
        .and_then(Item::as_str)
        .map(str::to_string)
}

fn short_tool_name(key: &str) -> &str {
    key.rsplit(['/', ':'])
        .next()
        .filter(|part| !part.is_empty())
        .unwrap_or(key)
}

fn write_atomic(path: &Path, contents: &str) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|dir| !dir.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "mise.toml".to_string());
    let tmp = parent.join(format!(".{}.{}.tmp", name, std::process::id()));
    std::fs::write(&tmp, contents).map_err(|e| e.to_string())?;
    let renamed = std::fs::rename(&tmp, path);
    if renamed.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    renamed.map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "toml_merge_tests.rs"]
mod tests;
