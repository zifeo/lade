use std::collections::HashMap;
use std::io::Cursor;

use crate::redact::Redactor;

const CONTEXT_NEEDLES: &[&[u8]] = &[
    b"authorization:",
    b"bearer ",
    b"x-api-key:",
    b"x-apikey:",
    b"api-key:",
    b"access_token=",
    b"api_key=",
    b"apikey=",
];

const AUTH_SCHEMES: &[&[u8]] = &[b"bearer ", b"token ", b"basic "];

const PREFIXES: &[&str] = &[
    "AKIA", "sk-", "sk_live_", "ghp_", "gho_", "xox", "eyJ", "glpat-",
];

pub(super) fn replace_known_values(command: &str, values: &HashMap<String, String>) -> String {
    let Some(redactor) = Redactor::new(values, "${{}}") else {
        return command.to_string();
    };
    let mut out = Vec::new();
    if redactor
        .stream(Cursor::new(command.as_bytes()), &mut out)
        .is_err()
    {
        return command.to_string();
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub(super) fn apply_context_needles(command: &str) -> String {
    let bytes = command.as_bytes();
    let lower: Vec<u8> = bytes.iter().map(|b| b.to_ascii_lowercase()).collect();
    let mut spans = Vec::new();
    for needle in CONTEXT_NEEDLES {
        let mut from = 0;
        while let Some(rel) = find_sub(&lower[from..], needle) {
            let after = from + rel + needle.len();
            let skip_schemes = *needle == b"authorization:";
            if let Some(span) = next_cred(&lower, after, skip_schemes) {
                spans.push(span);
            }
            from = after;
            if from >= lower.len() {
                break;
            }
        }
    }
    if spans.is_empty() {
        return command.to_string();
    }
    spans.sort_by_key(|&(start, _)| start);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for span in spans {
        if let Some(last) = merged.last_mut()
            && span.0 < last.1
        {
            last.1 = last.1.max(span.1);
            continue;
        }
        merged.push(span);
    }
    let mut out = String::new();
    let mut pos = 0;
    for (start, end) in merged {
        if start < pos {
            continue;
        }
        out.push_str(&command[pos..start]);
        out.push('?');
        pos = end;
    }
    out.push_str(&command[pos..]);
    out
}

fn find_sub(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn next_cred(hay: &[u8], after: usize, skip_schemes: bool) -> Option<(usize, usize)> {
    let mut i = after;
    while i < hay.len() && hay[i].is_ascii_whitespace() {
        i += 1;
    }
    if skip_schemes {
        for scheme in AUTH_SCHEMES {
            if hay[i..].starts_with(scheme) {
                i += scheme.len();
                break;
            }
        }
    }
    while i < hay.len() && hay[i].is_ascii_whitespace() {
        i += 1;
    }
    if i < hay.len() && hay[i] == b'$' {
        return None;
    }
    let start = i;
    while i < hay.len() {
        let b = hay[i];
        if b.is_ascii_whitespace() || matches!(b, b'"' | b'\'' | b'&' | b';') {
            break;
        }
        i += 1;
    }
    if i > start { Some((start, i)) } else { None }
}

pub(super) fn apply_prefix_and_length(command: &str) -> String {
    let mut out = String::new();
    let mut rest = command;
    while let Some((tok, tail, sep)) = next_token(rest) {
        out.push_str(&sep);
        if prefix_or_len_hit(tok) {
            out.push('?');
        } else {
            out.push_str(tok);
        }
        rest = tail;
    }
    out.push_str(rest);
    out
}

fn next_token(s: &str) -> Option<(&str, &str, String)> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() && is_token_sep(bytes[i]) {
        i += 1;
    }
    if i == bytes.len() {
        return None;
    }
    let start = i;
    while i < bytes.len() && !is_token_sep(bytes[i]) {
        i += 1;
    }
    Some((&s[start..i], &s[i..], s[..start].to_string()))
}

fn is_token_sep(b: u8) -> bool {
    b.is_ascii_whitespace() || matches!(b, b'=' | b':' | b'"' | b'\'')
}

fn prefix_or_len_hit(tok: &str) -> bool {
    if PREFIXES.iter().any(|p| tok.starts_with(p)) {
        return true;
    }
    if tok.chars().count() < 20 {
        return false;
    }
    tok.bytes().any(|b| b.is_ascii_digit()) && tok.bytes().any(|b| b.is_ascii_alphabetic())
}
