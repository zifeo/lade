#!/usr/bin/env bash
# Orchestrates the rendering of all tapes using render.py.
# Requires: docker, python3, asciinema, agg.

set -euo pipefail

tape_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$tape_dir/../.." && pwd)"
cd "$tape_dir"

COMPOSE_FILE="$repo_root/compose.yml"
COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-lade}"

cleanup() {
  echo "Cleaning up..."
  docker compose -f "$COMPOSE_FILE" -p "$COMPOSE_PROJECT_NAME" down >/dev/null 2>&1 || true
  rm -f .lade-test-config.json 2>/dev/null || true
}
trap cleanup EXIT

prepare_vault() {
  echo "Starting Vault..."
  if ! docker compose -f "$COMPOSE_FILE" -p "$COMPOSE_PROJECT_NAME" up -d vault >/dev/null; then
    echo "Error: Docker not running?" >&2
    exit 1
  fi

  # Wait for vault
  for i in $(seq 1 30); do
    if docker compose -f "$COMPOSE_FILE" -p "$COMPOSE_PROJECT_NAME" exec -T vault vault status >/dev/null 2>&1; then
      break
    fi
    sleep 1
  done

  # Initialize demo secrets
  docker compose -f "$COMPOSE_FILE" -p "$COMPOSE_PROJECT_NAME" exec -T \
    -e VAULT_ADDR=http://127.0.0.1:8200 \
    -e VAULT_TOKEN=token \
    vault vault kv put secret/password \
    value1=itsasecret \
    value2=itsanotsecret \
    multiline=$'a\nb' >/dev/null
}

# Ensure lade is built (already done in retrospective step)
# (cd "$repo_root" && cargo build)

prepare_vault

if [ $# -gt 0 ]; then
  for tape in "$@"; do
    python3 render.py "$tape"
  done
else
  python3 render.py
fi

echo "Done."
