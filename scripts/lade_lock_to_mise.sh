#!/usr/bin/env bash
# Print a mise.toml [tools] table from a Lade lock. Versions live in lade.lock.
set -euo pipefail

lock_path="${1:-lade.lock}"

if [[ ! -f "$lock_path" ]]; then
  echo "missing lock: $lock_path" >&2
  exit 1
fi

echo "[tools]"

name=""
version=""
backend=""

flush() {
  if [[ -z "$name" || -z "$version" ]]; then
    name=""
    version=""
    backend=""
    return 0
  fi
  local key="${backend:-$name}"
  printf '"%s" = "%s"\n' "$key" "$version"
  name=""
  version=""
  backend=""
}

while IFS= read -r line || [[ -n "$line" ]]; do
  if [[ "$line" =~ ^\[\[tools\.(.+)\]\]$ ]]; then
    flush
    name="${BASH_REMATCH[1]}"
    name="${name%\"}"
    name="${name#\"}"
    continue
  fi
  if [[ "$line" =~ ^version[[:space:]]*=[[:space:]]*\"(.*)\"$ ]]; then
    version="${BASH_REMATCH[1]}"
    continue
  fi
  if [[ "$line" =~ ^backend[[:space:]]*=[[:space:]]*\"(.*)\"$ ]]; then
    backend="${BASH_REMATCH[1]}"
    continue
  fi
done <"$lock_path"

flush
