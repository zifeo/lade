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
    *"64")
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
    *"64") PLATFORM='x86_64-apple-darwin' ;;
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
ASSET="$NAME-v$VERSION-$PLATFORM"
DOWNLOAD_URL="$RELEASE_URL/download/v$VERSION/$ASSET.$EXT"

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
# Older releases may not publish checksums; in that case we warn and continue.
if download "$DOWNLOAD_URL.sha256" "$TMP_DIR/$ASSET.$EXT.sha256"; then
  EXPECTED_SHA=$(cut -d' ' -f1 "$TMP_DIR/$ASSET.$EXT.sha256")
  if command -v sha256sum >/dev/null 2>&1; then
    ACTUAL_SHA=$(sha256sum "$TMP_DIR/$ASSET.$EXT" | cut -d' ' -f1)
  elif command -v shasum >/dev/null 2>&1; then
    ACTUAL_SHA=$(shasum -a 256 "$TMP_DIR/$ASSET.$EXT" | cut -d' ' -f1)
  else
    ACTUAL_SHA=""
    printf "Warning: no sha256 tool found, skipping checksum verification\n" >&2
  fi
  if [ -n "$ACTUAL_SHA" ]; then
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
  fi
else
  printf "Warning: no checksum published for %s, skipping verification\n" "$ASSET.$EXT" >&2
fi

tar -C "$TMP_DIR" -xzf "$TMP_DIR/$ASSET.$EXT" "$EXE"
chmod +x "$TMP_DIR/$EXE"

# need_confirm returns 0 only for interactive human terminals.
need_confirm() {
  if [ "${ASSUME_YES:-}" = "1" ]; then return 1; fi
  if [ -n "${CI:-}" ]; then return 1; fi
  if [ ! -t 0 ]; then return 1; fi
  return 0
}

if [ "${OUT_DIR}" = "." ]; then
  mv "$TMP_DIR/$EXE" .
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
    mv "$TMP_DIR/$EXE" "$OUT_DIR"
  else
    printf "Sudo is required to run \"sudo mv %s %s\":\n" "$TMP_DIR/$EXE" "$OUT_DIR"
    sudo mv "$TMP_DIR/$EXE" "$OUT_DIR"
  fi
fi

rm -r "$TMP_DIR"

OUT_DIR=$(realpath "$OUT_DIR")
case ":$PATH:" in
*":$OUT_DIR:"*) ;;
*)
  cat <<EOF

The installation directory is not in your PATH, consider adding it:
$ export PATH="\$PATH:$OUT_DIR"
Or moving the executable to another directory in your PATH:
$ sudo mv $EXE /usr/local/bin
EOF
  ;;
esac
