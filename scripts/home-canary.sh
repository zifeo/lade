#!/bin/sh
# Tests must never write to the real Lade or mise state under $HOME: a test
# that did rewrote a developer's mise shims and broke every agent shell.
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
    changed=$(
      for dir in \
        "$HOME/.config/lade" \
        "$HOME/.cache/lade" \
        "$HOME/.local/share/lade" \
        "$HOME/Library/Application Support/com.zifeo.lade" \
        "$HOME/Library/Caches/com.zifeo.lade" \
        "$HOME/.local/share/mise" \
        "$HOME/.local/state/mise"; do
        if [ -e "$dir" ]; then
          find "$dir" -newer "$marker" -print
        fi
      done
    )
    if [ -n "$changed" ]; then
      printf 'Tests wrote to the real HOME. Give them a temp HOME or the matching LADE_*/MISE_* dirs:\n%s\n' "$changed" >&2
      exit 1
    fi
    echo "real HOME untouched"
    ;;
  *)
    usage
    ;;
esac
