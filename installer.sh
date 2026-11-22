#!/bin/sh

set -e -u

ORG=zifeo
REPO=lade
EXT=tar.gz
NAME=lade
EXE=lade

INSTALLER_URL="${INSTALLER_URL:-https://raw.githubusercontent.com/$ORG/$REPO/main/installer.sh}"
RELEASE_URL="${RELEASE_URL:-https://github.com/$ORG/$REPO/releases}"

# Select a non-interactive downloader (curl preferred, wget fallback).
# DOWNLOADER may be forced to "curl" or "wget" via the environment.
DOWNLOADER="${DOWNLOADER:-}"
if [ -n "$DOWNLOADER" ]; then
  if ! command -v "$DOWNLOADER" >/dev/null 2>&1; then
    echo "Error: requested downloader '$DOWNLOADER' is not available." >&2
    exit 1
  fi
elif command -v curl >/dev/null 2>&1; then
  DOWNLOADER=curl
elif command -v wget >/dev/null 2>&1; then
  DOWNLOADER=wget
else
  echo "Error: neither curl nor wget is available, cannot download lade." >&2
  exit 1
fi

# download <url> <output-file>; returns non-zero on failure (never prompts).
download() {
  if [ "$DOWNLOADER" = "curl" ]; then
    curl --fail --silent --location --output "$2" "$1"
  else
    wget --quiet --output-document "$2" "$1"
  fi
}

# http_status downloads $1 to $2 and prints the final HTTP status, 000 when
# no response came (DNS, TLS, timeout).
http_status() {
  if [ "$DOWNLOADER" = "curl" ]; then
    curl --silent --location --output "$2" --write-out '%{http_code}' "$1" || true
  else
    wget --server-response --quiet --output-document "$2" "$1" 2>&1 |
      awk '$1 ~ /^HTTP\// { code = $2 } END { print (code == "" ? "000" : code) }'
  fi
}

# get_latest_version prints the latest tag version (without the leading v).
get_latest_version() {
  if [ "$DOWNLOADER" = "curl" ]; then
    _url=$(curl -s -L -I -o /dev/null -w '%{url_effective}' "$RELEASE_URL/latest")
  else
    # wget has no url_effective; read the redirect target from the `Location:`
    # response header (-S prints headers to stderr) and keep the last one.
    _url=$(wget -q -S --max-redirect=0 -O /dev/null "$RELEASE_URL/latest" 2>&1 | awk 'tolower($1) == "location:" { print $2 }' | tr -d '\r' | tail -n 1) || true
  fi
  echo "${_url##*v}"
}

PLATFORM="${PLATFORM:-}"
TMP_DIR=$(mktemp -d)
OUT_DIR="${OUT_DIR:-/usr/local/bin}"
VERSION="${VERSION:-$(get_latest_version)}"
MACHINE=$(uname -m)

if ldd --version 2>&1 | grep -q musl; then
  LIBC="musl"
else
  LIBC="gnu"
fi

if [ "${PLATFORM:-x}" = "x" ]; then
  # § mqnfssecp.b@6yzn2
  case "$(uname -s | tr '[:upper:]' '[:lower:]')" in
  "linux")
    case "$MACHINE" in
    "arm64"* | "aarch64"*)
      if [ "$LIBC" = "musl" ]; then
        PLATFORM='aarch64-unknown-linux-musl'
      else
        PLATFORM='aarch64-unknown-linux-gnu'
      fi
      ;;
    "x86_64" | "amd64")
      if [ "$LIBC" = "musl" ]; then
        PLATFORM='x86_64-unknown-linux-musl'
      else
        PLATFORM='x86_64-unknown-linux-gnu'
      fi
      ;;
    esac
    ;;
  "darwin")
    case "$MACHINE" in
    "arm64"* | "aarch64"*) PLATFORM='aarch64-apple-darwin' ;;
    "x86_64" | "amd64") PLATFORM='x86_64-apple-darwin' ;;
    esac
    ;;
  esac
  if [ "${PLATFORM:-x}" = "x" ]; then
    cat >&2 <<EOF

/!\\ We couldn't automatically detect your operating system. /!\\

To continue with installation, please choose from one of the following values:
- aarch64-unknown-linux-gnu
- aarch64-unknown-linux-musl
- x86_64-unknown-linux-gnu
- x86_64-unknown-linux-musl
- aarch64-apple-darwin
- x86_64-apple-darwin

Then set the PLATFORM environment variable, and re-run this script:
$ curl -fsSL $INSTALLER_URL | PLATFORM=x86_64-unknown-linux-musl bash
EOF
    exit 1
  fi
  printf "Detected platform: %s\n" "$PLATFORM"
fi

printf "Detected version: %s\n" "$VERSION"
# § tp05szjjy.a@wra3a
ASSET="$NAME-v$VERSION-$PLATFORM"
DOWNLOAD_URL="$RELEASE_URL/download/v$VERSION/$ASSET.$EXT"
# § end tp05szjjy.a

if download "$DOWNLOAD_URL" "$TMP_DIR/$ASSET.$EXT"; then
  printf "Downloaded successfully: %s\n" "$ASSET.$EXT"
else
  cat >&2 <<EOF

/!\\ The asset $ASSET.$EXT doesn't exist. /!\\

To continue with installation, please make sure the release exists in:
$RELEASE_URL

Then set the PLATFORM and VERSION environment variables, and re-run this script:
$ curl -fsSL $INSTALLER_URL | PLATFORM=x86_64-unknown-linux-musl VERSION=0.1.10 bash
EOF
  exit 1
fi

# Verify the SHA256 checksum published alongside the asset as <asset>.sha256.
# Older releases may not publish checksums: only a 404 means that. Any other
# failure may be a network that dropped it, so it stops the install.
SHA_STATUS=$(http_status "$DOWNLOAD_URL.sha256" "$TMP_DIR/$ASSET.$EXT.sha256")
case "$SHA_STATUS" in
2??)
  # § tp05szjjy.b@8jkxk
  EXPECTED_SHA=$(cut -d' ' -f1 "$TMP_DIR/$ASSET.$EXT.sha256")
  if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL_SHA=$(sha256sum "$TMP_DIR/$ASSET.$EXT" | cut -d' ' -f1)
  elif command -v shasum >/dev/null 2>&1; then
    ACTUAL_SHA=$(shasum -a 256 "$TMP_DIR/$ASSET.$EXT" | cut -d' ' -f1)
  else
    cat >&2 <<EOF

/!\\ Cannot verify the published checksum of $ASSET.$EXT /!\\

Neither sha256sum nor shasum is installed. Install one (coreutils or perl),
then re-run this script.
EOF
    rm -r "$TMP_DIR"
    exit 1
  fi
  if [ "$EXPECTED_SHA" = "$ACTUAL_SHA" ]; then
    printf "Checksum verified: %s\n" "$ACTUAL_SHA"
  else
    cat >&2 <<EOF

/!\\ Checksum verification failed for $ASSET.$EXT /!\\

Expected: $EXPECTED_SHA
Actual:   $ACTUAL_SHA

Aborting installation.
EOF
    rm -r "$TMP_DIR"
    exit 1
  fi
  ;;
404)
  # § tp05szjjy.e@p32vw
  printf "Warning: no checksum published (%s is missing), installing %s unverified\n" "$ASSET.$EXT.sha256" "$ASSET.$EXT" >&2
  ;;
*)
  # § tp05szjjy.b@e35y9
  cat >&2 <<EOF

/!\\ Could not download $ASSET.$EXT.sha256 (HTTP $SHA_STATUS) /!\\

Without it the asset cannot be verified. Aborting installation.
EOF
  rm -r "$TMP_DIR"
  exit 1
  ;;
esac

tar -C "$TMP_DIR" -xzf "$TMP_DIR/$ASSET.$EXT"
chmod +x "$TMP_DIR/$EXE"
if [ -f "$TMP_DIR/age-plugin-lade" ]; then
  chmod +x "$TMP_DIR/age-plugin-lade"
else
  # A release before 0.19.1 ships lade alone; lade upgrade warns the same way.
  printf "Warning: %s holds no age-plugin-lade, installing %s alone\n" "$ASSET.$EXT" "$EXE" >&2
fi

# need_confirm returns 0 only for interactive human terminals.
need_confirm() {
  # § tp05szjjy.d@wz6nx
  case "${ASSUME_YES:-}" in 1 | true) return 1 ;; esac
  # CI=false or CI=0 is not CI, as Lade itself reads it.
  case "$(printf '%s' "${CI:-}" | tr '[:upper:]' '[:lower:]')" in
  "" | 0 | false) ;;
  *) return 1 ;;
  esac
  if [ ! -t 0 ]; then return 1; fi
  return 0
}

install_extracted() {
  # § tp05szjjy.c@7qw7q
  dest_dir="$1"
  run="${2-}"
  $run mv "$TMP_DIR/$EXE" "$dest_dir"
  if [ -f "$TMP_DIR/age-plugin-lade" ]; then
    $run mv "$TMP_DIR/age-plugin-lade" "$dest_dir/age-plugin-lade"
  fi
}

if [ "${OUT_DIR}" = "." ]; then
  install_extracted .
  printf "\n\n%s has been extracted to your current directory\n" "$EXE"
else
  cat <<EOF

$EXE will be moved to $OUT_DIR
Set the OUT_DIR environment variable to change the installation directory:
$ curl -fsSL $INSTALLER_URL | OUT_DIR=. bash

EOF
  if [ -w "${OUT_DIR}" ]; then
    if need_confirm; then
      printf "Press enter to continue (or cancel with Ctrl+C):"
      read -r _
    fi
    install_extracted "$OUT_DIR"
  else
    printf "Sudo is required to run \"sudo mv %s %s\":\n" "$TMP_DIR/$EXE" "$OUT_DIR"
    install_extracted "$OUT_DIR" sudo
  fi
fi

rm -r "$TMP_DIR"

# `lade setup` drops every profile line that holds `lade-do-not-edit` when it
# writes its hook, so this marker must never contain that text.
PATH_MARKER="lade-path-do-not-edit"

OUT_DIR=$(realpath "$OUT_DIR")
# § tp05szjjy.c@8qd01
case ":$PATH:" in
*":$OUT_DIR:"*) ;;
*)
  # The startup file `lade setup` writes its hook to for this login shell.
  SHELL_NAME="${SHELL:-}"
  case "${SHELL_NAME##*/}" in
  bash) PROFILE="$HOME/.bashrc" ;;
  zsh) PROFILE="$HOME/.zshrc" ;;
  fish) PROFILE="$HOME/.config/fish/config.fish" ;;
  *) PROFILE="$HOME/.profile" ;;
  esac
  case "$PROFILE" in
  *.fish) PATH_LINE="set --global --export PATH \$PATH '$OUT_DIR'  # $PATH_MARKER" ;;
  *) PATH_LINE="export PATH=\"\$PATH:$OUT_DIR\"  # $PATH_MARKER" ;;
  esac
  if [ -f "$PROFILE" ] && {
    grep -qxF "$PATH_LINE" "$PROFILE" ||
      grep -F "$OUT_DIR" "$PROFILE" | grep -vF "$PATH_MARKER" | grep -q PATH
  }; then
    printf "\n%s already adds %s to PATH. Open a new shell to use %s.\n" "$PROFILE" "$OUT_DIR" "$EXE"
  else
    mkdir -p "$(dirname "$PROFILE")"
    if [ -f "$PROFILE" ] && grep -qF "$PATH_MARKER" "$PROFILE"; then
      KEPT=$(grep -vF "$PATH_MARKER" "$PROFILE" || true)
      if [ -n "$KEPT" ]; then printf '%s\n' "$KEPT" >"$PROFILE"; else : >"$PROFILE"; fi
    elif [ -s "$PROFILE" ] && [ -n "$(tail -c 1 "$PROFILE")" ]; then
      printf '\n' >>"$PROFILE"
    fi
    printf '%s\n' "$PATH_LINE" >>"$PROFILE"
    cat <<EOF

Added $OUT_DIR to PATH in $PROFILE. Open a new shell, or run:
$ export PATH="\$PATH:$OUT_DIR"
EOF
  fi
  ;;
esac
# § end tp05szjjy.c
