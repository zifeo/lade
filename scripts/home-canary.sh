#!/bin/sh
# Tests must never write to the real Lade or mise state under $HOME: a test
# that did rewrote a developer's mise shims and broke every agent shell.
# That state includes the home mise config and lock `lade setup` writes
# when it runs in $HOME (src/mise/plane.rs).
# `mark FILE` before the tests, `check FILE` after: it lists what changed
# since the mark (a directory's mtime also catches deletions). Meant for CI,
# where nothing else writes there meanwhile.
set -eu

usage() {
  echo "usage: $0 mark|check FILE" >&2
  exit 2
}

[ "$#" -eq 2 ] || usage
marker=$2

case "$1" in
  mark)
    : >"$marker"
    ;;
  check)
    [ -f "$marker" ] || {
      echo "no marker at $marker: run '$0 mark $marker' before the tests" >&2
      exit 2
    }
    # § ft4f31hbf.a@1921t
    changed=$(
      for dir in \
        "$HOME/.config/lade" \
        "$HOME/.cache/lade" \
        "$HOME/.local/share/lade" \
        "$HOME/Library/Application Support/com.zifeo.lade" \
        "$HOME/Library/Caches/com.zifeo.lade" \
        "$HOME/.local/share/mise" \
        "$HOME/.local/state/mise" \
        "$HOME/.cache/mise" \
        "$HOME/.config/mise" \
        "$HOME/.config/mise.toml" \
        "$HOME/mise" \
        "$HOME/.mise" \
        "$HOME/mise.toml" \
        "$HOME/.mise.toml" \
        "$HOME/mise.lock" \
        "$HOME/lade.lock"; do
        if [ -e "$dir" ]; then
          find "$dir" -newer "$marker" -print
        fi
      done
    )
    if [ -n "$changed" ]; then
      printf 'Tests wrote to the real HOME. Give them a temp HOME or the matching LADE_*/MISE_* dirs:\n%s\n' "$changed" >&2
      exit 1
    fi
    # § end ft4f31hbf.a
    echo "real HOME untouched"
    ;;
  *)
    usage
    ;;
esac
