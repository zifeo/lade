//! Argv-shape classification of a shell command for the pre-tool hook.
//!
//! Never reads yaml `log`, match, or `needs_wrap`. First positional after
//! the lade binary and global flags decides.

use crate::preexec::CHILD_UNSET;
use crate::ticket;

/// Clap verbs except `inject`, which is a vehicle.
const VERBS: &[&str] = &[
    "add", "approve", "bench", "cache", "eval", "help", "hook", "hub", "log", "mcp", "off", "on",
    "remove", "set", "setup", "status", "teardown", "unset", "update", "upgrade", "usage", "user",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Class {
    NotLade,
    Empty,
    Verb,
    Finished,
    Vehicle { inner: String },
}

impl Class {
    pub(super) fn inner(self) -> Option<String> {
        match self {
            Class::Vehicle { inner } => Some(inner),
            _ => None,
        }
    }
}

pub(super) fn classify(command: &str) -> Class {
    let mut current = command.trim().to_string();
    for _ in 0..8 {
        match classify_once(&current) {
            Class::Vehicle { inner } if inner != current => current = inner,
            other => {
                if matches!(other, Class::NotLade) && current != command.trim() {
                    return Class::Vehicle { inner: current };
                }
                return other;
            }
        }
    }
    classify_once(&current)
}

fn classify_once(command: &str) -> Class {
    let mut rest = command.trim_start();
    let Some((prog, after)) = split_token(rest) else {
        return Class::Empty;
    };
    if !is_lade_bin(&prog) {
        return Class::NotLade;
    }
    rest = after.trim_start();
    let mut ticket_id: Option<String> = None;
    loop {
        let Some((tok, after)) = split_token(rest) else {
            return Class::Empty;
        };
        if tok == "--pretool" {
            rest = after.trim_start();
            if let Some((next, after_next)) = split_token(rest)
                && ticket::exists(&next)
            {
                ticket_id = Some(next);
                rest = after_next.trim_start();
            }
            continue;
        }
        if let Some(id) = tok.strip_prefix("--pretool=") {
            if ticket::is_id(id) {
                ticket_id = Some(id.to_string());
            }
            rest = after.trim_start();
            continue;
        }
        if matches!(
            tok.as_str(),
            "--via=pretool"
                | "--no-mask"
                | "-v"
                | "-vv"
                | "-q"
                | "-h"
                | "--help"
                | "--version"
                | "--verbose"
        ) {
            rest = after.trim_start();
            continue;
        }
        if tok == "--via" || tok == "--mask-format" {
            rest = after.trim_start();
            if let Some((_, after_next)) = split_token(rest) {
                rest = after_next.trim_start();
            }
            continue;
        }
        if tok == "--" {
            rest = after.trim_start();
            break;
        }
        if tok == "inject" {
            rest = after.trim_start();
            if let Some((next, after_next)) = split_token(rest)
                && next == "--"
            {
                rest = after_next.trim_start();
            }
            break;
        }
        if is_verb(&tok) {
            return Class::Verb;
        }
        break;
    }
    let inner = unescape_hook_word(rest.trim());
    if inner.is_empty() {
        return Class::Empty;
    }
    if let Some(id) = ticket_id.as_deref()
        && ticket_finished(id, &inner)
    {
        return Class::Finished;
    }
    Class::Vehicle { inner }
}

fn ticket_finished(id: &str, inner: &str) -> bool {
    ticket::read(id).is_ok_and(|pre| pre.command == inner)
}

fn is_verb(tok: &str) -> bool {
    VERBS.contains(&tok)
}

fn is_lade_bin(prog: &str) -> bool {
    super::bin::lade_basename(std::path::Path::new(prog)).is_some()
}

pub(super) fn sanitize_env_prefix(prefix: &str) -> String {
    prefix
        .split_whitespace()
        .filter(|assign| {
            assign
                .split_once('=')
                .is_none_or(|(name, _)| !CHILD_UNSET.contains(&name))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn join_prefix(prefix: &str, command: &str) -> String {
    if prefix.is_empty() {
        command.to_string()
    } else {
        format!("{prefix} {command}")
    }
}

pub(super) fn wrap_ticket(bin: &str, id: &str, inner: &str) -> String {
    format!(
        "{} --pretool={} '{}'",
        bin,
        id,
        inner.replace('\'', "'\\''")
    )
}

pub(super) fn wrap_idless(bin: &str, inner: &str) -> String {
    format!("{} --pretool -- '{}'", bin, inner.replace('\'', "'\\''"))
}

fn split_token(s: &str) -> Option<(String, &str)> {
    let s = s.trim_start();
    if s.is_empty() {
        return None;
    }
    let bytes = s.as_bytes();
    let quote = match bytes[0] {
        b @ (b'\'' | b'"') => Some(b),
        _ => None,
    };
    if let Some(q) = quote {
        let close = s[1..].find(q as char).map(|i| i + 1)?;
        Some((s[1..close].to_string(), &s[close + 1..]))
    } else {
        let end = s.find(char::is_whitespace).unwrap_or(s.len());
        Some((s[..end].to_string(), &s[end..]))
    }
}

/// Inverse of `wrap_ticket` / `wrap_idless` quoting: outer `'` plus `'\''`.
fn unescape_hook_word(s: &str) -> String {
    if s.len() >= 2 && s.starts_with('\'') && s.ends_with('\'') {
        return s[1..s.len() - 1].replace("'\\''", "'");
    }
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        return s[1..s.len() - 1].to_string();
    }
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verbs_are_not_vehicles() {
        assert_eq!(classify("lade status"), Class::Verb);
        assert_eq!(classify("lade hook"), Class::Verb);
        assert_eq!(classify("lade --pretool mcp -- acme"), Class::Verb);
        assert_eq!(classify("lade usage | grep inject"), Class::Verb);
        assert_eq!(classify("echo lade inject"), Class::NotLade);
    }

    #[test]
    fn vehicles_peel_to_inner() {
        assert_eq!(
            classify("lade inject -- tofu apply"),
            Class::Vehicle {
                inner: "tofu apply".into()
            }
        );
        assert_eq!(
            classify("lade --pretool inject -- tofu apply"),
            Class::Vehicle {
                inner: "tofu apply".into()
            }
        );
        assert_eq!(
            classify("lade -- tofu apply"),
            Class::Vehicle {
                inner: "tofu apply".into()
            }
        );
        assert_eq!(
            classify("lade tofu apply"),
            Class::Vehicle {
                inner: "tofu apply".into()
            }
        );
        assert_eq!(
            classify("lade inject -- lade inject -- tofu"),
            Class::Vehicle {
                inner: "tofu".into()
            }
        );
        assert_eq!(classify("lade inject"), Class::Empty);
        assert_eq!(
            classify("/usr/bin/lade.exe inject echo"),
            Class::Vehicle {
                inner: "echo".into()
            }
        );
    }

    #[test]
    fn fake_ticket_id_is_vehicle() {
        assert_eq!(
            classify("lade --pretool=x7Km 'echo hello'"),
            Class::Vehicle {
                inner: "echo hello".into()
            }
        );
    }

    #[test]
    fn wrap_ticket_quotes_round_trip() {
        let inner = "echo 'hi'";
        let wrapped = wrap_ticket("lade", "Ab12", inner);
        assert_eq!(
            classify(&wrapped),
            Class::Vehicle {
                inner: inner.into()
            }
        );
    }
}
