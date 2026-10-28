//! Where a hook command runs, read statically: the payload's directory, then
//! the command's leading `cd <dir> &&` segments.

use serde_json::Value;
use std::path::{Component, Path, PathBuf};

/// A word with one of these is not a literal path: expansion, quoting, or a
/// shell operator.
const NOT_LITERAL: &[char] = &[
    '$', '`', '\'', '"', '\\', ';', '|', '&', '<', '>', '(', ')', '\n', '\r',
];

/// The harness's own directory for this command. Claude Code and Codex send
/// the session `cwd`, Cursor sends `cwd` and `tool_input.working_directory`,
/// a Codex shell call may add `tool_input.workdir` relative to `cwd`.
/// `fallback` covers payloads with neither (the OpenCode plugin).
pub(super) fn payload_dir(input: &Value, fallback: &Path) -> PathBuf {
    let tool = input.get("tool_input");
    let mut dir = fallback.to_path_buf();
    if let Some(cwd) = text(input.get("cwd")) {
        dir = dir.join(cwd);
    }
    if let Some(work) =
        text(tool.and_then(|t| t.get("workdir").or_else(|| t.get("working_directory"))))
    {
        dir = dir.join(work);
    }
    lexical(&dir)
}

fn text(value: Option<&Value>) -> Option<&str> {
    value.and_then(Value::as_str).filter(|s| !s.is_empty())
}

/// The whole command is one `cd`, `pushd`, or `popd` with at most one
/// literal argument. Wrapped, it would move lade's child and not the
/// harness shell.
pub(super) fn is_shell_move(command: &str) -> bool {
    let mut words = command.split_whitespace();
    let Some(verb) = words.next() else {
        return false;
    };
    let args: Vec<&str> = words.collect();
    matches!(verb, "cd" | "pushd" | "popd")
        && !command.contains(['\n', '\r'])
        && args.len() <= 1
        && args.iter().all(|arg| literal(arg) && !is_option(arg))
}

/// Applies leading `cd <dir> &&` segments to `start`. Stops at the first
/// segment it cannot resolve, so the directory is the last one resolved.
/// Returns the directory and the command left after the applied segments.
pub(crate) fn run_dir<'a>(start: &Path, command: &'a str) -> (PathBuf, &'a str) {
    let mut dir = start.to_path_buf();
    let mut rest = command.trim_start();
    while let Some((head, tail)) = rest.split_once("&&") {
        let Some(next) = cd_target(&dir, head) else {
            break;
        };
        dir = next;
        rest = tail.trim_start();
    }
    (dir, rest)
}

/// The same directory, through symlinks: a harness may send `/tmp/x` while
/// `getcwd` says `/private/tmp/x`.
pub(crate) fn same_dir(a: &Path, b: &Path) -> bool {
    a == b
        || matches!(
            (a.canonicalize(), b.canonicalize()),
            (Ok(a), Ok(b)) if a == b
        )
}

fn cd_target(base: &Path, segment: &str) -> Option<PathBuf> {
    let mut words = segment.split_whitespace();
    let (Some("cd"), Some(word), None) = (words.next(), words.next(), words.next()) else {
        return None;
    };
    if segment.contains(['\n', '\r']) || !literal(word) || word.starts_with('-') {
        return None;
    }
    // With `CDPATH`, `cd sub` may land in a CDPATH entry, not `./sub`.
    let searched = !word.starts_with(['/', '.', '~']);
    if searched && std::env::var_os("CDPATH").is_some_and(|v| !v.is_empty()) {
        return None;
    }
    let path = match word.strip_prefix('~') {
        Some("") => crate::home_dir()?,
        Some(tail) => crate::home_dir()?.join(tail.strip_prefix('/')?),
        None => base.join(word),
    };
    let path = lexical(&path);
    path.is_dir().then_some(path)
}

fn literal(word: &str) -> bool {
    !word.contains(NOT_LITERAL)
}

fn is_option(word: &str) -> bool {
    word.starts_with('-') && word != "-"
}

/// `cd` resolves `..` on the path as typed, not on symlink targets.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}
