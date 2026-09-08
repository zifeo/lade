set -e

export LADE_VAULT_HTTP=1

if ! curl -s http://127.0.0.1:8200/v1/sys/health > /dev/null; then
  docker compose up -d vault
fi

echo "Checking Vault accessibility..."
for _ in {1..100}; do
  if curl -s http://127.0.0.1:8200/v1/sys/health > /dev/null; then
    echo "Vault is accessible."
    break
  fi
  sleep 2
done

if ! curl -s http://127.0.0.1:8200/v1/sys/health > /dev/null; then
  echo "Error: Vault is not accessible after multiple attempts."
  exit 1
fi

vault_put() {
  local secret_path="$1"
  local payload="$2"
  curl -sS -f \
    -H "X-Vault-Token: ${VAULT_TOKEN:-token}" \
    -H "Content-Type: application/json" \
    -X POST \
    "http://127.0.0.1:8200/v1/secret/data/${secret_path}" \
    --data "$payload" >/dev/null
}

vault_put password '{"data":{"value1":"itsasecret","value2":"itsanotsecret","multiline":"a\\nb"}}'
vault_put org/team '{"data":{"value":"secret"}}'
