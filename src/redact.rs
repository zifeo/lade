use aho_corasick::{AhoCorasick, AhoCorasickBuilder, MatchKind};
use rustc_hash::FxHashMap;
use std::{
    collections::HashMap,
    io::{self, Read, Write},
};

const CHUNK: usize = 64 * 1024;

pub struct Redactor {
    ac: AhoCorasick,
    replacements: Vec<Vec<u8>>,
    // carry_len = max_pattern_len - 1: how many trailing bytes to defer to the
    // next chunk so a secret that spans a read boundary is never missed.
    carry_len: usize,
}

impl Redactor {
    /// `format` controls the replacement token; `{}` is substituted with the
    /// variable name. Omit `{}` for a static replacement (e.g. `"REDACTED"`).
    /// Returns `None` when there are no non-empty secrets to mask.
    pub fn new(secrets: &HashMap<String, String>, format: &str) -> Option<Self> {
        let mut by_value: FxHashMap<&str, &str> = FxHashMap::default();
        let mut names: Vec<&str> = secrets.keys().map(String::as_str).collect();
        names.sort_unstable();
        for name in &names {
            let value = secrets[*name].as_str();
            if !value.is_empty() {
                by_value.entry(value).or_insert(name);
            }
        }
        if by_value.is_empty() {
            return None;
        }

        // Longer patterns first so LeftmostLongest picks the longer one when
        // a shorter secret is a prefix of a longer one.
        let mut pairs: Vec<(&str, &str)> = by_value.into_iter().collect();
        pairs.sort_unstable_by(|(a, _), (b, _)| b.len().cmp(&a.len()).then(a.cmp(b)));

        let carry_len = pairs
            .iter()
            .map(|(v, _)| v.len())
            .max()
            .unwrap_or(0)
            .saturating_sub(1);

        let mut patterns: Vec<Vec<u8>> = Vec::with_capacity(pairs.len());
        let mut replacements: Vec<Vec<u8>> = Vec::with_capacity(pairs.len());
        for (value, name) in pairs {
            patterns.push(value.as_bytes().to_vec());
            replacements.push(format.replace("{}", name).into_bytes());
        }

        let ac = AhoCorasickBuilder::new()
            .match_kind(MatchKind::LeftmostLongest)
            .build(&patterns)
            .expect("aho-corasick build failed");

        Some(Self {
            ac,
            replacements,
            carry_len,
        })
    }

    pub fn stream<R: Read, W: Write>(&self, mut reader: R, mut writer: W) -> io::Result<()> {
        let mut buf: Vec<u8> = Vec::new();
        let mut chunk = vec![0u8; CHUNK];

        loop {
            let n = reader.read(&mut chunk)?;
            if n == 0 {
                self.replace_into(&buf, &mut writer)?;
                break;
            }

            buf.extend_from_slice(&chunk[..n]);

            // Only commit bytes before safe_end; a match spanning the boundary
            // is still processed in full, with the tail kept as carry.
            let safe_end = buf.len().saturating_sub(self.carry_len);
            let mut pos = 0usize;

            for mat in self.ac.find_iter(&buf) {
                if mat.start() >= safe_end {
                    break;
                }
                writer.write_all(&buf[pos..mat.start()])?;
                writer.write_all(&self.replacements[mat.pattern().as_usize()])?;
                pos = mat.end();
            }

            let commit = pos.max(safe_end);
            writer.write_all(&buf[pos..commit])?;
            buf.drain(..commit);
        }
        Ok(())
    }

    fn replace_into(&self, data: &[u8], writer: &mut impl Write) -> io::Result<()> {
        let mut pos = 0;
        for mat in self.ac.find_iter(data) {
            writer.write_all(&data[pos..mat.start()])?;
            writer.write_all(&self.replacements[mat.pattern().as_usize()])?;
            pos = mat.end();
        }
        writer.write_all(&data[pos..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    const DEFAULT_FMT: &str = "${{}:-REDACTED}";

    fn redact_fmt(secrets: &[(&str, &str)], input: &[u8], fmt: &str) -> Vec<u8> {
        let map: HashMap<String, String> = secrets
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let r = Redactor::new(&map, fmt).expect("expected a redactor");
        let mut out = Vec::new();
        r.stream(Cursor::new(input), &mut out).unwrap();
        out
    }

    fn redact(secrets: &[(&str, &str)], input: &[u8]) -> Vec<u8> {
        redact_fmt(secrets, input, DEFAULT_FMT)
    }

    #[test]
    fn test_basic_replacement() {
        let out = redact(&[("KEY", "abc123")], b"prefix abc123 suffix");
        assert_eq!(out, b"prefix ${KEY:-REDACTED} suffix");
    }

    #[test]
    fn test_empty_value_ignored() {
        let map: HashMap<String, String> =
            [("KEY".to_string(), "".to_string())].into_iter().collect();
        assert!(Redactor::new(&map, DEFAULT_FMT).is_none());
    }

    #[test]
    fn test_all_empty_returns_none() {
        assert!(Redactor::new(&HashMap::new(), DEFAULT_FMT).is_none());
    }

    #[test]
    fn test_duplicate_value_picks_sorted_first_name() {
        // Both vars share the same value; alphabetically first name wins.
        let out = redact(&[("B_VAR", "secret"), ("A_VAR", "secret")], b"secret");
        assert_eq!(out, b"${A_VAR:-REDACTED}");
    }

    #[test]
    fn test_longer_pattern_wins_over_prefix() {
        let out = redact(&[("SHORT", "abc"), ("LONG", "abcdef")], b"abcdef");
        assert_eq!(out, b"${LONG:-REDACTED}");
    }

    #[test]
    fn test_static_format() {
        let out = redact_fmt(&[("KEY", "abc123")], b"abc123 abc123", "REDACTED");
        assert_eq!(out, b"REDACTED REDACTED");
    }

    #[test]
    fn test_custom_format_with_name() {
        let out = redact_fmt(&[("KEY", "abc123")], b"abc123", "[REDACTED:{}]");
        assert_eq!(out, b"[REDACTED:KEY]");
    }

    #[test]
    fn test_boundary_split_across_reads() {
        // One-byte-at-a-time reader forces the carry buffer to reassemble a
        // secret that spans consecutive read calls.
        struct OneByte<'a> {
            data: &'a [u8],
            pos: usize,
        }
        impl Read for OneByte<'_> {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                if self.pos >= self.data.len() {
                    return Ok(0);
                }
                buf[0] = self.data[self.pos];
                self.pos += 1;
                Ok(1)
            }
        }
        let map: HashMap<String, String> = [("TOKEN".to_string(), "abcdef".to_string())]
            .into_iter()
            .collect();
        let r = Redactor::new(&map, DEFAULT_FMT).unwrap();
        let mut out = Vec::new();
        r.stream(
            OneByte {
                data: b"xabcdefx",
                pos: 0,
            },
            &mut out,
        )
        .unwrap();
        assert_eq!(out, b"x${TOKEN:-REDACTED}x");
    }

    #[test]
    fn test_multiple_occurrences() {
        let out = redact(&[("K", "s3cr3t")], b"s3cr3t foo s3cr3t");
        assert_eq!(out, b"${K:-REDACTED} foo ${K:-REDACTED}");
    }
}
