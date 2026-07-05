#!/usr/bin/env bash
# Local/unit test for installer.sh.
#
# Serves a fake release asset + checksum over a local HTTP server and runs
# installer.sh against it (via the RELEASE_URL override) to verify:
#   - shellcheck cleanliness (when shellcheck is available)
#   - platform pinning via PLATFORM
#   - version pinning via VERSION
#   - SHA256 checksum verification (positive and negative cases)
#   - missing checksum is tolerated (backward compatibility)
#   - non-interactive mode does not hang (CI=1, no TTY)
#
# Usage: bash tests/installer_test.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
INSTALLER="$REPO_ROOT/installer.sh"
PLATFORM="${PLATFORM:-x86_64-unknown-linux-musl}"
VERSION="0.0.0-test"
ASSET="lade-v$VERSION-$PLATFORM"

WORK="$(mktemp -d)"
SERVE="$WORK/serve"
ASSET_DIR="$SERVE/download/v$VERSION"
SERVER_PID=""

cleanup() {
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

pass() { printf "ok - %s\n" "$1"; }
fail() {
  printf "FAIL - %s\n" "$1" >&2
  exit 1
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

# --- 1. shellcheck ---------------------------------------------------------
if command -v shellcheck >/dev/null 2>&1; then
  if shellcheck "$INSTALLER"; then pass "shellcheck clean"; else fail "shellcheck reported issues"; fi
else
  fail "shellcheck not installed"
fi

# --- build fake asset ------------------------------------------------------
mkdir -p "$ASSET_DIR"
BIN_DIR="$WORK/bin"
mkdir -p "$BIN_DIR"
cat >"$BIN_DIR/lade" <<'EOF'
#!/bin/sh
echo "lade 0.0.0-test"
EOF
chmod +x "$BIN_DIR/lade"
tar -C "$BIN_DIR" -czf "$ASSET_DIR/$ASSET.tar.gz" lade
sha256_of "$ASSET_DIR/$ASSET.tar.gz" >"$ASSET_DIR/$ASSET.tar.gz.sha256"

# --- start HTTP server -----------------------------------------------------
PORT=8731
python3 -m http.server "$PORT" --directory "$SERVE" >/dev/null 2>&1 &
SERVER_PID=$!
disown "$SERVER_PID" 2>/dev/null || true
for _ in $(seq 1 50); do
  if curl -fsS "http://127.0.0.1:$PORT/" >/dev/null 2>&1; then break; fi
  sleep 0.1
done

BASE="http://127.0.0.1:$PORT"

run_installer() {
  out_dir="$1"
  shift
  mkdir -p "$out_dir"
  env RELEASE_URL="$BASE" PLATFORM="$PLATFORM" VERSION="$VERSION" \
    OUT_DIR="$out_dir" CI=1 "$@" sh "$INSTALLER" </dev/null
}

# --- 2. positive install (checksum verified) for curl and wget -------------
for dl in curl wget; do
  if ! command -v "$dl" >/dev/null 2>&1; then
    fail "$dl not installed"
  fi
  OUT1="$WORK/out1-$dl"
  if run_installer "$OUT1" DOWNLOADER="$dl" >"$WORK/log1-$dl" 2>&1; then
    [ -x "$OUT1/lade" ] || fail "binary not installed in positive case ($dl)"
    grep -q "Checksum verified" "$WORK/log1-$dl" || fail "checksum was not verified ($dl)"
    "$OUT1/lade" | grep -q "0.0.0-test" || fail "installed binary does not run ($dl)"
    pass "positive install with checksum verification ($dl)"
  else
    cat "$WORK/log1-$dl" >&2
    fail "installer failed in positive case ($dl)"
  fi
done

# --- 3. negative install (bad checksum aborts) -----------------------------
echo "deadbeef00000000000000000000000000000000000000000000000000000000" >"$ASSET_DIR/$ASSET.tar.gz.sha256"
OUT2="$WORK/out2"
if run_installer "$OUT2" >"$WORK/log2" 2>&1; then
  fail "installer should have aborted on bad checksum"
else
  grep -qi "Checksum verification failed" "$WORK/log2" || fail "missing checksum failure message"
  [ ! -e "$OUT2/lade" ] || fail "binary should not be installed on bad checksum"
  pass "negative install aborts on bad checksum"
fi

# --- 4. missing checksum tolerated -----------------------------------------
rm -f "$ASSET_DIR/$ASSET.tar.gz.sha256"
OUT3="$WORK/out3"
if run_installer "$OUT3" >"$WORK/log3" 2>&1; then
  [ -x "$OUT3/lade" ] || fail "binary not installed when checksum missing"
  grep -qi "no checksum published" "$WORK/log3" || fail "missing checksum warning absent"
  pass "missing checksum tolerated with warning"
else
  cat "$WORK/log3" >&2
  fail "installer failed when checksum missing"
fi

printf "\nAll installer tests passed.\n"
