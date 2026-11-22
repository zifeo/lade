#!/usr/bin/env bash
# Local/unit test for installer.sh.
#
# Serves a fake release asset + checksum over a local HTTP server and runs
# installer.sh against it (via the RELEASE_URL override) to verify:
#   - shellcheck cleanliness (when shellcheck is available)
#   - platform pinning via PLATFORM
#   - version pinning via VERSION
#   - SHA256 checksum verification (positive and negative cases)
#   - missing checksum is tolerated with a warning naming it (backward compatibility)
#   - a checksum the server fails to serve aborts
#   - a published checksum with no sha256 tool aborts
#   - an unknown 64-bit machine stops before any download
#   - non-interactive mode does not hang (CI=1, no TTY)
#   - under a terminal, ASSUME_YES=true asks nothing and CI=false still asks
#   - OUT_DIR off PATH is added once to the login shell's profile, beside the
#     `lade on` line, and a profile already adding it is left alone
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
cp "$BIN_DIR/lade" "$BIN_DIR/age-plugin-lade"
chmod +x "$BIN_DIR/lade" "$BIN_DIR/age-plugin-lade"
tar -C "$BIN_DIR" -czf "$ASSET_DIR/$ASSET.tar.gz" lade age-plugin-lade
sha256_of "$ASSET_DIR/$ASSET.tar.gz" >"$ASSET_DIR/$ASSET.tar.gz.sha256"

# --- start HTTP server -----------------------------------------------------
PORT=8731
# Serves files like http.server, and a 503 for any path with a sibling <path>.fail.
cat >"$WORK/server.py" <<'EOF'
import functools
import http.server
import os
import sys


class Handler(http.server.SimpleHTTPRequestHandler):
    def send_head(self):
        if os.path.exists(self.translate_path(self.path) + ".fail"):
            self.send_error(503)
            return None
        return super().send_head()


handler = functools.partial(Handler, directory=sys.argv[2])
http.server.ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), handler).serve_forever()
EOF
python3 -I "$WORK/server.py" "$PORT" "$SERVE" >"$WORK/server.log" 2>&1 &
SERVER_PID=$!
disown "$SERVER_PID" 2>/dev/null || true
for _ in $(seq 1 50); do
  if curl -fsS "http://127.0.0.1:$PORT/" >/dev/null 2>&1; then break; fi
  sleep 0.1
done

BASE="http://127.0.0.1:$PORT"

# The installer writes the shell profile under HOME, so every run gets a
# HOME of its own; a test passes HOME= or SHELL= after it to override them.
run_installer() {
  out_dir="$1"
  shift
  mkdir -p "$out_dir" "$out_dir.home"
  env RELEASE_URL="$BASE" PLATFORM="$PLATFORM" VERSION="$VERSION" \
    OUT_DIR="$out_dir" CI=1 HOME="$out_dir.home" SHELL=/bin/bash "$@" sh "$INSTALLER" </dev/null
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
  grep -q "$ASSET.tar.gz.sha256 is missing" "$WORK/log3" || fail "warning does not name the missing checksum"
  pass "missing checksum tolerated with warning"
else
  cat "$WORK/log3" >&2
  fail "installer failed when checksum missing"
fi

sha256_of "$ASSET_DIR/$ASSET.tar.gz" >"$ASSET_DIR/$ASSET.tar.gz.sha256"

# --- 4b. a checksum the server fails to serve aborts -------------------------
touch "$ASSET_DIR/$ASSET.tar.gz.sha256.fail"
for dl in curl wget; do
  OUT4="$WORK/out4-$dl"
  if run_installer "$OUT4" DOWNLOADER="$dl" >"$WORK/log4-$dl" 2>&1; then
    cat "$WORK/log4-$dl" >&2
    fail "installer should abort when the checksum download fails ($dl)"
  else
    grep -q "Could not download $ASSET.tar.gz.sha256 (HTTP 503)" "$WORK/log4-$dl" ||
      fail "checksum download failure not named ($dl)"
    [ ! -e "$OUT4/lade" ] || fail "binary should not be installed unverified ($dl)"
    pass "a failed checksum download aborts ($dl)"
  fi
done
rm -f "$ASSET_DIR/$ASSET.tar.gz.sha256.fail"

# --- 4c. a release without age-plugin-lade installs lade alone, with a warning
mv "$ASSET_DIR/$ASSET.tar.gz" "$WORK/full.tar.gz"
tar -C "$BIN_DIR" -czf "$ASSET_DIR/$ASSET.tar.gz" lade
sha256_of "$ASSET_DIR/$ASSET.tar.gz" >"$ASSET_DIR/$ASSET.tar.gz.sha256"
OUT4C="$WORK/out4c"
if run_installer "$OUT4C" >"$WORK/log4c" 2>&1; then
  [ -x "$OUT4C/lade" ] || fail "binary not installed from a release without the plugin"
  [ ! -e "$OUT4C/age-plugin-lade" ] || fail "age-plugin-lade appeared from nowhere"
  grep -q "holds no age-plugin-lade" "$WORK/log4c" || fail "missing plugin warning absent"
  pass "a release without age-plugin-lade installs lade alone with a warning"
else
  cat "$WORK/log4c" >&2
  fail "installer failed on a release without age-plugin-lade"
fi
mv "$WORK/full.tar.gz" "$ASSET_DIR/$ASSET.tar.gz"
sha256_of "$ASSET_DIR/$ASSET.tar.gz" >"$ASSET_DIR/$ASSET.tar.gz.sha256"

# --- 5. published checksum and no sha256 tool aborts ------------------------
TOOLS="$WORK/tools"
mkdir -p "$TOOLS"
for tool in sh curl tar gzip cut mktemp uname tr grep chmod mv realpath rm cat ldd; do
  if path="$(command -v "$tool" 2>/dev/null)" && [ "${path#/}" != "$path" ]; then
    ln -s "$path" "$TOOLS/$tool"
  fi
done
OUT5="$WORK/out5"
if run_installer "$OUT5" PATH="$TOOLS" >"$WORK/log5" 2>&1; then
  cat "$WORK/log5" >&2
  fail "installer should abort when it cannot compute a published checksum"
else
  grep -q "Neither sha256sum nor shasum" "$WORK/log5" || fail "missing tool message absent"
  [ ! -e "$OUT5/lade" ] || fail "binary should not be installed without a sha256 tool"
  pass "published checksum without a sha256 tool aborts"
fi

# --- 6. unknown 64-bit machine stops before any download --------------------
STUB="$WORK/stub-uname"
mkdir -p "$STUB"
cat >"$STUB/uname" <<'EOF'
#!/bin/sh
case "$1" in
-m) echo riscv64 ;;
*) echo Linux ;;
esac
EOF
chmod +x "$STUB/uname"
OUT6="$WORK/out6"
: >"$WORK/server.log"
if run_installer "$OUT6" PLATFORM= PATH="$STUB:$PATH" >"$WORK/log6" 2>&1; then
  fail "installer should refuse an unknown machine"
else
  grep -q "couldn't automatically detect" "$WORK/log6" || fail "missing detection message for riscv64"
  if grep -q "$VERSION" "$WORK/server.log"; then fail "an asset was requested for riscv64"; fi
  [ ! -e "$OUT6/lade" ] || fail "binary should not be installed for riscv64"
  pass "unknown 64-bit machine refused before download"
fi

# --- 7, 8. prompt under a terminal: ASSUME_YES=true skips it, CI=false asks --
# at_pty <log> <env...>: runs the installer with a terminal as stdin. Its
# input ends at once, so a prompt reads end of input and never hangs the test.
at_pty() {
  log="$1"
  shift
  out_dir="$WORK/out-pty-$(basename "$log")"
  mkdir -p "$out_dir" "$out_dir.home"
  cmd="env -u CI -u ASSUME_YES RELEASE_URL=$BASE PLATFORM=$PLATFORM VERSION=$VERSION OUT_DIR=$out_dir HOME=$out_dir.home SHELL=/bin/bash $* sh $INSTALLER"
  if [ "$(uname -s)" = "Darwin" ]; then
    # shellcheck disable=SC2086
    script -q /dev/null $cmd </dev/null >"$log" 2>&1 || true
  else
    script -qec "$cmd" /dev/null </dev/null >"$log" 2>&1 || true
  fi
}
at_pty "$WORK/log7" ASSUME_YES=true
if grep -q "Press enter" "$WORK/log7"; then fail "ASSUME_YES=true still prompted"; fi
[ -x "$WORK/out-pty-log7/lade" ] || {
  cat "$WORK/log7" >&2
  fail "ASSUME_YES=true did not install under a terminal"
}
pass "ASSUME_YES=true asks nothing"
at_pty "$WORK/log8" CI=false
grep -q "Press enter" "$WORK/log8" || fail "CI=false should still prompt a terminal"
pass "CI=false is not CI"

# --- 9. OUT_DIR off PATH is added to the login shell's profile, once ---------
OUT9="$WORK/out9"
HOME9="$WORK/home9"
mkdir -p "$OUT9" "$HOME9"
DIR9="$(realpath "$OUT9")"
HOOK_LINE='eval "$(lade on)"  # lade-do-not-edit'
printf 'alias ll="ls -l"\n%s\n' "$HOOK_LINE" >"$HOME9/.zshrc"
run_installer "$OUT9" HOME="$HOME9" SHELL=/bin/zsh >"$WORK/log9" 2>&1 || {
  cat "$WORK/log9" >&2
  fail "installer failed with OUT_DIR off PATH"
}
PATH_LINE9="export PATH=\"\$PATH:$DIR9\"  # lade-path-do-not-edit"
[ "$(grep -cxF "$PATH_LINE9" "$HOME9/.zshrc")" = 1 ] || {
  cat "$HOME9/.zshrc" >&2
  fail "the first run did not add OUT_DIR to ~/.zshrc"
}
grep -qxF "$HOOK_LINE" "$HOME9/.zshrc" || fail "the first run dropped the lade on line"
grep -qxF 'alias ll="ls -l"' "$HOME9/.zshrc" || fail "the first run dropped a line of the user"
grep -q "Added $DIR9 to PATH in $HOME9/.zshrc" "$WORK/log9" || fail "the first run did not say where it added OUT_DIR"
grep -F "lade-path-do-not-edit" "$HOME9/.zshrc" >"$WORK/path9.sh"
[ "$(env PATH=/usr/bin:/bin sh -c ". '$WORK/path9.sh'; command -v lade")" = "$DIR9/lade" ] ||
  fail "the added line does not put OUT_DIR on PATH"
pass "OUT_DIR off PATH is added to the profile beside the lade on line"
cp "$HOME9/.zshrc" "$WORK/zshrc9"
run_installer "$OUT9" HOME="$HOME9" SHELL=/bin/zsh >"$WORK/log9b" 2>&1 || fail "the second run failed"
cmp -s "$HOME9/.zshrc" "$WORK/zshrc9" || {
  diff "$WORK/zshrc9" "$HOME9/.zshrc" >&2 || true
  fail "the second run changed the profile"
}
grep -q "already adds $DIR9 to PATH" "$WORK/log9b" || fail "the second run did not say the profile already adds OUT_DIR"
pass "a second run changes nothing"

OUT9F="$WORK/out9f"
run_installer "$OUT9F" SHELL=/usr/bin/fish >"$WORK/log9f" 2>&1 || fail "installer failed for fish"
grep -qxF "set --global --export PATH \$PATH '$(realpath "$OUT9F")'  # lade-path-do-not-edit" \
  "$OUT9F.home/.config/fish/config.fish" || fail "fish profile line missing"
OUT9S="$WORK/out9s"
run_installer "$OUT9S" SHELL=/bin/sh >"$WORK/log9s" 2>&1 || fail "installer failed for sh"
grep -qF "lade-path-do-not-edit" "$OUT9S.home/.profile" || fail "another shell's line is not in ~/.profile"
pass "fish and other shells get the profile lade setup uses"

OUT9O="$WORK/out9o"
mkdir -p "$OUT9O" "$OUT9O.home"
# shellcheck disable=SC2016
printf 'export PATH="%s:$PATH"\n' "$(realpath "$OUT9O")" >"$OUT9O.home/.bashrc"
cp "$OUT9O.home/.bashrc" "$WORK/bashrc9o"
run_installer "$OUT9O" >"$WORK/log9o" 2>&1 || fail "installer failed with a profile already adding OUT_DIR"
cmp -s "$OUT9O.home/.bashrc" "$WORK/bashrc9o" || fail "a profile already adding OUT_DIR was edited"
pass "a profile already holding OUT_DIR on PATH is left alone"

OUT9P="$WORK/out9p"
mkdir -p "$OUT9P"
run_installer "$OUT9P" PATH="$(realpath "$OUT9P"):$PATH" >"$WORK/log9p" 2>&1 || fail "installer failed with OUT_DIR on PATH"
[ ! -e "$OUT9P.home/.bashrc" ] || fail "OUT_DIR on PATH still wrote a profile"
pass "OUT_DIR on PATH writes no profile"

printf "\nAll installer tests passed.\n"
