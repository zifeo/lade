#!/usr/bin/env bash
# Orchestrates the rendering of all tapes using render.py.
# Requires: docker, python3, asciinema, agg.

set -euo pipefail

tape_dir="$(cd "$(dirname "$0")" && pwd)"
repo_root="$(cd "$tape_dir/../.." && pwd)"
cd "$tape_dir"

COMPOSE_FILE="$repo_root/compose.yml"
COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-lade}"
K3D_CLUSTER="${LADE_K3D_CLUSTER:-lade-k3d-shared}"
K3D_CONTEXT="k3d-$K3D_CLUSTER"
K3D_CONFIG_FILE="$repo_root/k3d.yaml"
K3D_MANIFESTS_FILE="$repo_root/k3d-manifests.yaml"
TAPE_LADE_YML="$tape_dir/lade.yml"

cleanup() {
  echo "Cleaning up..."
  docker compose -f "$COMPOSE_FILE" -p "$COMPOSE_PROJECT_NAME" down >/dev/null 2>&1 || true
  rm -f .lade-test-config.json 2>/dev/null || true
}
trap cleanup EXIT

prepare_vault() {
  if docker compose -f "$COMPOSE_FILE" -p "$COMPOSE_PROJECT_NAME" exec -T vault vault status >/dev/null 2>&1; then
    # Check if a demo secret already exists to avoid re-initializing
    if docker compose -f "$COMPOSE_FILE" -p "$COMPOSE_PROJECT_NAME" exec -T \
      -e VAULT_ADDR=http://127.0.0.1:8200 \
      -e VAULT_TOKEN=token \
      vault vault kv get secret/password >/dev/null 2>&1; then
      return
    fi
  fi

  echo "Starting Vault..."
  if ! docker compose -f "$COMPOSE_FILE" -p "$COMPOSE_PROJECT_NAME" up -d vault >/dev/null; then
    echo "Error: Docker not running?" >&2
    exit 1
  fi

  # Wait for vault
  for _ in $(seq 1 30); do
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

prepare_k3d() {
  if ! command -v k3d >/dev/null 2>&1; then
    echo "Error: k3d is required for network tape rendering." >&2
    exit 1
  fi
  if ! command -v kubectl >/dev/null 2>&1; then
    echo "Error: kubectl is required for network tape rendering." >&2
    exit 1
  fi
  if ! docker info >/dev/null 2>&1; then
    echo "Error: Docker must be running for K3D." >&2
    exit 1
  fi

  local needs_apply=0
  if ! kubectl config get-contexts -o name | rg -x "$K3D_CONTEXT" >/dev/null 2>&1; then
    echo "Creating K3D cluster..."
    k3d cluster create --config "$K3D_CONFIG_FILE" --wait >/dev/null
    needs_apply=1
  fi

  if [ "${LADE_RENDER_RECREATE_K3D:-0}" = "1" ]; then
    echo "Recreating K3D cluster..."
    k3d cluster delete "$K3D_CLUSTER" >/dev/null 2>&1 || true
    k3d cluster create --config "$K3D_CONFIG_FILE" --wait >/dev/null
    needs_apply=1
  fi

  if [ "$needs_apply" -eq 1 ] || ! kubectl --context "$K3D_CONTEXT" -n lade-k3d-ns get deployment http-echo >/dev/null 2>&1; then
    echo "Applying K3D manifests..."
    kubectl --context "$K3D_CONTEXT" apply -f "$K3D_MANIFESTS_FILE" >/dev/null
    kubectl --context "$K3D_CONTEXT" -n lade-k3d-ns rollout status deployment/http-echo --timeout=120s >/dev/null
  fi
}

sync_network_tape_config() {
  local server_url
  server_url="$(kubectl --context "$K3D_CONTEXT" config view --raw -o "jsonpath={.clusters[?(@.name==\"$K3D_CONTEXT\")].cluster.server}")"
  if [ -z "$server_url" ]; then
    echo "Error: failed to resolve kube API server for $K3D_CONTEXT." >&2
    exit 1
  fi
  local authority
  authority="${server_url#https://}"
  authority="${authority#http://}"
  authority="${authority%%/*}"
  python3 - "$TAPE_LADE_YML" "$authority" "$K3D_CONTEXT" <<'PY'
import re
import sys

path, authority, context = sys.argv[1], sys.argv[2], sys.argv[3]
with open(path, "r", encoding="utf-8") as f:
    text = f.read()

pattern = r"(?ms)^# tape: network\n.*?(?=^# tape: |\Z)"
replacement = (
    "# tape: network\n"
    "^curl .*127.0.0.1:18080/.*$:\n"
    "  TF_VAR_demo_token: file://../sources/config.json?query=.token\n"
    f"  18080: kubectl://{authority}/{context}/lade-k3d-ns/service/http-echo/8080\n"
)
if not re.search(pattern, text):
    print("missing '# tape: network' section", file=sys.stderr)
    sys.exit(1)
text = re.sub(pattern, replacement, text)
with open(path, "w", encoding="utf-8") as f:
    f.write(text)
PY
}

# Ensure lade is built and up to date
echo "Building lade..."
(cd "$repo_root" && cargo build --release >/dev/null)

needs_network=0
needs_vault=1
if [ $# -eq 0 ]; then
  needs_network=1
else
  if [ $# -eq 1 ] && [ "$1" = "network" ]; then
    needs_vault=0
  fi
  for tape in "$@"; do
    if [ "$tape" = "network" ]; then
      needs_network=1
      break
    fi
  done
fi

if [ "$needs_vault" -eq 1 ]; then
  prepare_vault
fi
if [ "$needs_network" -eq 1 ]; then
  prepare_k3d
  sync_network_tape_config
fi

if [ $# -gt 0 ]; then
  for tape in "$@"; do
    python3 render.py "$tape"
  done
else
  python3 render.py
fi

echo "Done."
