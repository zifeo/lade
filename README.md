# Lade

- https://github.com/hwchen/keyring-rs

doppler://api.doppler.com/project/env/var

doppler --api-host https://api.doppler.com run --project cloud --config prd
--mount secrets.json -- cat secrets.json

op://Personal/gitlab_token/password

echo "test: op://Personal/gitlab_token/password" | op inject

infisical://app.infisical.com/workspace-63a2290a0edf8bf1f65e3784/env/var

{ "workspaceId": "63a2290a0edf8bf1f65e3784" }

infisical --domain https://app.infisical.com/api export --env=dev --format=json
