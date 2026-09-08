use std::ffi::OsString;

use age_plugin::run_state_machine;
use anyhow::Result;

mod handler;
mod link;
mod parse;

pub use handler::PLUGIN_NAME;
pub(crate) use link::copy_beside;
use link::is_plugin_argv0;

pub enum Invoke {
    StateMachine(String),
    Encode(String),
}

pub fn invoke_from_argv(argv: &[OsString]) -> Option<Invoke> {
    if argv.is_empty() {
        return None;
    }
    let named = is_plugin_argv0(&argv[0]);
    let mut sm = None;
    let mut positionals = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let a = argv[i].to_str().unwrap_or("");
        if let Some(rest) = a.strip_prefix("--age-plugin=") {
            sm = Some(rest.to_owned());
        } else if a == "--age-plugin" {
            sm = argv.get(i + 1).and_then(|s| s.to_str()).map(str::to_owned);
            i += 1;
        } else if !a.starts_with('-') {
            positionals.push(a.to_owned());
        }
        i += 1;
    }
    if let Some(sm) = sm {
        return Some(Invoke::StateMachine(sm));
    }
    if named {
        return positionals.first().cloned().map(Invoke::Encode);
    }
    None
}

pub fn run(invoke: Invoke) -> Result<()> {
    match invoke {
        Invoke::StateMachine(sm) => {
            run_state_machine(&sm, handler::Handler).map_err(|e| anyhow::anyhow!(e))?;
        }
        Invoke::Encode(uri) => {
            age_plugin::print_new_identity(PLUGIN_NAME, uri.as_bytes(), uri.as_bytes());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(args: &[&str]) -> Vec<OsString> {
        args.iter().map(OsString::from).collect()
    }

    #[test]
    fn intercepts_equals_form() {
        let v = invoke_from_argv(&os(&["lade", "--age-plugin=identity-v1"])).unwrap();
        assert!(matches!(v, Invoke::StateMachine(s) if s == "identity-v1"));
    }

    #[test]
    fn intercepts_separate_form() {
        let v = invoke_from_argv(&os(&["lade", "--age-plugin", "recipient-v1"])).unwrap();
        assert!(matches!(v, Invoke::StateMachine(s) if s == "recipient-v1"));
    }

    #[test]
    fn named_binary_without_uri_is_none() {
        assert!(invoke_from_argv(&os(&["age-plugin-lade"])).is_none());
    }

    #[test]
    fn named_binary_encodes_uri() {
        let v =
            invoke_from_argv(&os(&["age-plugin-lade", "file:///tmp/k.json?query=.key"])).unwrap();
        assert!(matches!(v, Invoke::Encode(u) if u == "file:///tmp/k.json?query=.key"));
    }

    #[test]
    fn normal_lade_argv_is_ignored() {
        assert!(invoke_from_argv(&os(&["lade", "eval", "op://v/i/f"])).is_none());
        assert!(invoke_from_argv(&os(&["lade", "install"])).is_none());
    }
}
