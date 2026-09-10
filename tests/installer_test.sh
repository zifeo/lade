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
AGENT_SETUP="$REPO_ROOT/agent-setup.sh"
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
  if shellcheck "$INSTALLER" "$AGENT_SETUP"; then pass "shellcheck clean"; else fail "shellcheck reported issues"; fi
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
cp "$BIN_DIR/lade" "$BIN_DIR/age-plugin-lade"
chmod +x "$BIN_DIR/lade" "$BIN_DIR/age-plugin-lade"
tar -C "$BIN_DIR" -czf "$ASSET_DIR/$ASSET.tar.gz" lade age-plugin-lade
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
    [ -x "$OUT1/age-plugin-lade" ] || fail "age-plugin-lade missing ($dl)"
    [ ! -L "$OUT1/age-plugin-lade" ] || fail "age-plugin-lade should be a real binary, not a symlink ($dl)"
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
  [ -x "$OUT3/age-plugin-lade" ] || fail "age-plugin-lade missing when checksum missing"
  [ ! -L "$OUT3/age-plugin-lade" ] || fail "age-plugin-lade should be a real binary, not a symlink"
  grep -qi "no checksum published" "$WORK/log3" || fail "missing checksum warning absent"
  pass "missing checksum tolerated with warning"
else
  cat "$WORK/log3" >&2
  fail "installer failed when checksum missing"
fi

# --- 5. agent-setup.sh uses an existing lade, then calls setup --------------
PATH_DIR="$WORK/path"
mkdir -p "$PATH_DIR"
cat >"$PATH_DIR/lade" <<'EOF'
#!/bin/sh
printf 'setup-called %s\n' "$*" >"${LADE_MARKER}"
EOF
chmod +x "$PATH_DIR/lade"
MARKER="$WORK/setup-marker"
if env PATH="$PATH_DIR:$PATH" LADE_MARKER="$MARKER" sh "$AGENT_SETUP" >"$WORK/log-agent-existing" 2>&1; then
  grep -q "setup-called setup" "$MARKER" || fail "agent-setup did not run lade setup"
  pass "agent-setup runs setup when lade is on PATH"
else
  cat "$WORK/log-agent-existing" >&2
  fail "agent-setup failed when lade was already on PATH"
fi

# --- 6. agent-setup.sh installs then runs setup ----------------------------
path_without_lade() {
  _out=""
  _old_ifs=$IFS
  IFS=:
  for _dir in $PATH; do
    if [ -n "$_dir" ] && [ ! -x "$_dir/lade" ]; then
      if [ -z "$_out" ]; then
        _out="$_dir"
      else
        _out="$_out:$_dir"
      fi
    fi
  done
  IFS=$_old_ifs
  printf '%s\n' "$_out"
}

OUT4="$WORK/out4"
mkdir -p "$OUT4"
# After installer, PATH must see OUT4/lade. The fake binary from the asset
# ignores argv, so setup still exits 0.
if env RELEASE_URL="$BASE" PLATFORM="$PLATFORM" VERSION="$VERSION" \
  OUT_DIR="$OUT4" CI=1 INSTALLER_FILE="$INSTALLER" \
  PATH="$OUT4:$(path_without_lade)" \
  sh "$AGENT_SETUP" </dev/null >"$WORK/log-agent-install" 2>&1; then
  [ -x "$OUT4/lade" ] || fail "agent-setup did not install lade"
  pass "agent-setup installs then runs setup"
else
  cat "$WORK/log-agent-install" >&2
  fail "agent-setup failed when lade was missing"
fi

printf "\nAll installer tests passed.\n"
