#!/usr/bin/env bash
# Bump the workspace crate version.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <X.Y.Z> | --bump <level>" >&2
  exit 1
fi

if [[ "$1" == "--bump" ]]; then
  if [[ $# -lt 2 ]]; then
    echo "usage: $0 --bump <level>" >&2
    exit 1
  fi
  cargo set-version --workspace --bump "$2"
else
  cargo set-version --workspace "$1"
fi

version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
if [[ -z "$version" ]]; then
  echo "could not read version from Cargo.toml" >&2
  exit 1
fi

printf '%s\n' "$version"
