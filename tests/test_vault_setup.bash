set -e

docker compose up -d

echo "Checking Vault accessibility..."
for i in {1..100}; do
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

vault kv put -address=http://127.0.0.1:8200 -mount=secret password value1=itsasecret value2=itsanotsecret multiline="a\nb"
vault kv put -address=http://127.0.0.1:8200 -mount=secret org/team value=secret
