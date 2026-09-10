#!/bin/sh
# Put lade on PATH if needed, then wire this directory (`lade setup`).
# Always non-interactive. Run from a git repo so agent hooks land in the repo.

set -e -u

ORG=zifeo
REPO=lade
INSTALLER_URL="${INSTALLER_URL:-https://raw.githubusercontent.com/$ORG/$REPO/main/installer.sh}"
OUT_DIR="${OUT_DIR:-/usr/local/bin}"

export CI="${CI:-1}"
export ASSUME_YES="${ASSUME_YES:-1}"

lade_bin() {
  if command -v lade >/dev/null 2>&1; then
    command -v lade
    return 0
  fi
  if [ -x "$OUT_DIR/lade" ]; then
    printf '%s\n' "$OUT_DIR/lade"
    return 0
  fi
  return 1
}

run_installer() {
  if [ -n "${INSTALLER_FILE:-}" ]; then
    sh "$INSTALLER_FILE"
    return
  fi
  if command -v curl >/dev/null 2>&1; then
    curl --fail --silent --location "$INSTALLER_URL" | sh
    return
  fi
  if command -v wget >/dev/null 2>&1; then
    wget --quiet --output-document=- "$INSTALLER_URL" | sh
    return
  fi
  printf "Error: neither curl nor wget is available, cannot download lade.\n" >&2
  exit 1
}

if ! bin=$(lade_bin); then
  run_installer
  bin=$(lade_bin) || {
    printf "Error: lade is not on PATH after install.\n" >&2
    exit 1
  }
fi

exec "$bin" setup
