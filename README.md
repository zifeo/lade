# Lade

![Crates.io](https://img.shields.io/crates/v/lade)

Temporary access to secrets, files, and private networks for one command,
then gone. Same wrap for humans and agents. See which access was used.

<p align="center">
  <img src="./examples/tape/main.gif" alt="Demo" />
</p>

Lade (/leɪd/) on [Fish](https://fishshell.com),
[Bash](https://www.gnu.org/software/bash/), or [Zsh](https://zsh.sourceforge.io).
macOS and Linux. Secrets from [1Password CLI](https://1password.com/downloads/command-line/),
[Infisical](https://infisical.com), [Doppler](https://www.doppler.com),
[Vault](https://github.com/hashicorp/vault),
[Passbolt](https://www.passbolt.com),
[AWS Secrets Manager](https://docs.aws.amazon.com/secretsmanager/),
[Azure Key Vault](https://learn.microsoft.com/azure/key-vault/),
[GCP Secret Manager](https://cloud.google.com/secret-manager),
[age](https://github.com/FiloSottile/age), [SOPS](https://github.com/getsops/sops),
files, shell commands, or inline values.
Forwards through `kubectl`, `kubefwd`, Teleport `tsh`, or SSH, only while the
command runs. Also CI, Cursor, Claude Code, Codex, and OpenCode.

## Getting started

```bash
curl -fsSL https://raw.githubusercontent.com/zifeo/lade/main/installer.sh | bash
lade install
```

`lade install` writes pre-exec for this shell and pre-tool for detected
agents (hook and skill together). Then write a `lade.yml` (next section).
Pause and resume pre-exec with `lade off` and `lade on`.

Alternative installs:

```bash
cargo install lade --locked
cargo install --git https://github.com/zifeo/lade --locked
```

Upgrade with:

```bash
lade upgrade
```

## How it works

Create a `lade.yml` at your project root. Lade walks from the current
directory up to `$HOME` and merges every `lade.yml` it finds. Each
top-level key is a regular expression matched against the command
being run.

```yaml
"psql .*":
  DB_USER: op://my.1password.com/eng/postgres/username
  DB_PORT: kubectl://k8s.example.com:6443/prod/default/service/postgres/5432
  DATABASE_URL: postgres://${DB_USER}@127.0.0.1:${DB_PORT}/app
```

Now run the command normally:

```bash
psql "$DATABASE_URL"
```

Lade resolves `DB_USER`, opens a local forward for `DB_PORT`, interpolates both
into `DATABASE_URL`, runs the command, masks resolved secret values from output,
and cleans up when `psql` exits.

Pre-exec is the recommended path: you keep typing the command. Without it,
prefix with `lade`. The explicit form is `lade inject <command>`.

```bash
lade terraform apply
lade inject -- terraform apply
```

The wrap skips the user profile. Same argv as `sh://`.

## Observability

Opt-in. A matching rule with `log: true` records that the command ran and
which public keys and vault URIs it used. Values are never stored. `lade
usage` lists the rules that actually fired. Unused rules are omitted.
Details: [docs/observability.md](docs/observability.md).

```yaml
.:
  .:
    log: true
"^git status":
  .:
    log: false
"^npm run deploy":
  API_TOKEN: op://prod/api/credential
```

```bash
lade log
lade log --json --since 7d --until 1d --limit 20
lade log --group command
lade log --kind access --audience human
lade usage
lade usage --since 7d
lade usage --all
lade usage --path ~/other/repo
lade log --all
lade log prune --keep 30d
```

Default window: last 90 days. `--since` / `--until` are durations back from
now (`Ns | Nm | Nh | Nd | Nw | Nmonth`, `m` is minutes). `--limit` is an
extra cap. Bare `--limit 20` drops the 90-day default. `--group command`
counts by command text. Nothing prunes by itself. `lade log --help` prints
the database path. `lade status` prints path, count, and size.

Queries stay on the current git root (worktrees count). `--all` reads every
repo. `--path` scopes to another tree. `lade log` is typed commands. `lade
usage` is matched rules in this tree, most frequent first, with the file
path and `env` / `file` / `tunnel`.

## Common patterns

<table>
<tr>
<td width="50%">

**pre-exec** - You type the command in this shell. A `lade.yml` match gets
secrets and tunnels for that process only.

</td>
<td width="50%">

![pre-exec](./examples/tape/hooks.gif)

</td>
</tr>
<tr>
<td width="50%">

**Provider resolution** - Only the matching rule's URIs load. Other vaults
stay closed.

</td>
<td width="50%">

![Provider resolution](./examples/tape/resolution.gif)

</td>
</tr>
<tr>
<td width="50%">

**Manual injection** - No pre-exec? Prefix with `lade`. Same wrap as
`lade inject --`.

</td>
<td width="50%">

![Manual injection](./examples/tape/inject.gif)

</td>
</tr>
<tr>
<td width="50%">

**Private networks** - Local forward lives for the process. A numeric key is
a fixed port.

</td>
<td width="50%">

![Private network](./examples/tape/network.gif)

</td>
</tr>
<tr>
<td width="50%">

**Secrets as files** - `.file` writes a temp JSON/YAML, then deletes it.

</td>
<td width="50%">

![Secrets as files](./examples/tape/file-output.gif)

</td>
</tr>
<tr>
<td width="50%">

**Per-user values** - One `lade.yml`. `lade user alice` picks the map key.

</td>
<td width="50%">

![Per-user secrets](./examples/tape/per-user.gif)

</td>
</tr>
<tr>
<td width="50%">

**Human approval** - `disclaimer` withholds access until `lade approve <code>`.

</td>
<td width="50%">

![Disclaimer](./examples/tape/disclaimer.gif)

</td>
</tr>
<tr>
<td width="50%">

**Shell command provider** - `sh://` stdout is a secret.

</td>
<td width="50%">

![Shell command provider](./examples/tape/shell.gif)

</td>
</tr>
<tr>
<td width="50%">

**Intermediate bindings** - `.TOKEN` builds another value. The child never
sees `.TOKEN`.

</td>
<td width="50%">

![Intermediate bindings](./examples/tape/intermediate.gif)

</td>
</tr>
</table>

## AI agents

The agent types the command and never sees provider-resolved secrets.
Prefer this repo so clones share the guard. To wire a hook by hand:

```bash
lade hook install --harness cursor
lade hook install --scope user --harness cursor
```

`--scope user` is this machine (`CODEX_HOME` for Codex). `--harness` is
required in the files. Auto-detect is a safety net. `lade status` prints
`run \`lade install\`` on drift.

The equivalent project configs are:

<details>
<summary>Cursor (<code>.cursor/hooks.json</code>)</summary>

```json
{
  "version": 1,
  "hooks": {
    "preToolUse": [
      {
        "command": "lade hook --harness cursor",
        "matcher": "Shell"
      }
    ]
  }
}
```

</details>

<details>
<summary>Claude Code (<code>.claude/settings.json</code>)</summary>

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "lade hook --harness claude"
          }
        ]
      }
    ]
  }
}
```

</details>

<details>
<summary>Codex (<code>.codex/hooks.json</code>)</summary>

Trust the Lade command in `/hooks`. An untrusted hook or
`[features].hooks = false` is a silent no-op. User file:
`~/.codex/hooks.json`.

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "lade hook --harness codex"
          }
        ]
      }
    ]
  }
}
```

</details>

<details>
<summary>OpenCode (<code>.opencode/plugins/lade-pretool.js</code>)</summary>

Native OpenCode loads plugins, not Claude-style `hooks.json`. User:
`~/.config/opencode/plugins/`. The plugin runs `lade hook` on
`tool.execute.before` and applies the rewritten command.

```js
import { spawnSync } from "node:child_process";

const lade = process.env.LADE_BIN ?? "lade";

export const LadePretool = async () => ({
  "tool.execute.before": async (input, output) => {
    const command = output.args?.command;
    if (input.tool !== "bash" || typeof command !== "string") {
      return;
    }
    const result = spawnSync(lade, ["hook", "--harness", "opencode"], {
      input: JSON.stringify({ command, session_id: input.sessionID }),
      encoding: "utf8",
    });
    if (result.status !== 0 || !result.stdout?.trim()) {
      return;
    }
    try {
      const updated = JSON.parse(result.stdout)?.command;
      if (typeof updated === "string") {
        output.args.command = updated;
      }
    } catch {}
  },
});
```

</details>

### APM

```bash
apm install zifeo/lade#v0.17.2
```

That pin is a GitHub tag. It installs the skill
([`.agents/skills/lade/SKILL.md`](.agents/skills/lade/SKILL.md)).
Hooks stay in the files above or `lade hook install`.

### Agents without pre-tool

Add a short instruction to `AGENTS.md`:

```text
When a command needs access defined in lade.yml, prefix it with lade.
Example: lade terraform apply
```

Pre-tool is preferred: the agent does not guess which commands match.

## MCP

A desktop MCP client launches a server without your shell's secret manager.
`lade mcp` is the command the client runs. It hydrates only for that
connection, then exits and cleans up when the connection closes.

Add a server entry in your MCP client's configuration. Use the absolute path to
the installed `lade` binary when a GUI application does not inherit your shell
`PATH`.

For a local stdio server, all public mappings become child environment
variables:

```json
{
  "command": "lade",
  "args": ["mcp", "--", "acme-mcp", "--stdio"]
}
```

Match the canonical server command in `lade.yml`:

```yaml
"^acme-mcp --stdio$":
  API_TOKEN: op://company/acme/api-token
```

For a remote Streamable HTTP server, public mapping keys become HTTP header
names. The URL itself is the matcher:

```yaml
"^https://mcp\\.secureframe\\.com/$":
  .API_KEY: op://company/secureframe/api-key
  .API_SECRET: op://company/secureframe/api-secret
  Authorization: "${API_KEY} ${API_SECRET}"
```

```json
{
  "command": "lade",
  "args": ["mcp", "https://mcp.secureframe.com/"]
}
```

`.NAME` is an intermediate binding (see below). Here `.API_KEY` never
becomes a header.

To troubleshoot an MCP connection, add `-v` before `mcp` in the client
configuration arguments. Lade writes action-only traces to stderr, such as
`mcp http -> tools/call` and `mcp http <- 200 (42 ms)`. It never logs headers,
JSON-RPC parameters, request bodies, or resolved values. Use `-vv` for debug
logs; `LADE_LOG` overrides the command-line verbosity.

## Configuration reference

Lade has two provider families used from the same `lade.yml` rule:

- Secret providers resolve values into environment variables or temporary files.
- Network providers open a local forward for the process, then close it.

### Secrets

```yaml
"terraform .*":
  TF_VAR_api_key: op://DOMAIN/VAULT/ITEM/FIELD
```

Secret providers use an HTTP or cloud SDK when that keeps the same batch.
CLI stays when there is no API, or when the CLI is the only one-call export.
Authenticate the token or CLI first. Provider-resolved values are masked
unless `--no-mask` is set. Inline values are not masked: they are already
visible in `lade.yml`.

Supported secret providers:

| Provider      | URI                                                  | Notes                                               |
| ------------- | ---------------------------------------------------- | --------------------------------------------------- |
| 1Password     | `op://DOMAIN/VAULT/ITEM/FIELD`                       | Optional section: `op://DOMAIN/VAULT/ITEM/SECTION/FIELD`. Uses the 1Password CLI. |
| Infisical     | `infisical://DOMAIN/PROJECT_ID/ENV_NAME/SECRET_NAME` | Nested folders before the name. HTTP, `INFISICAL_TOKEN` / `INFISICAL_API_TOKEN` / `LADE_INFISICAL_TOKEN`. `LADE_INFISICAL_HTTP` for http. |
| Doppler       | `doppler://DOMAIN/PROJECT_NAME/ENV_NAME/SECRET_NAME` | Uses the Doppler CLI.                               |
| Vault         | `vault://DOMAIN/MOUNT/KEY/FIELD`                     | Path segments are URL-decoded. HTTP KV v2 only. `VAULT_TOKEN` or `LADE_VAULT_TOKEN`. `VAULT_NAMESPACE` / `LADE_VAULT_NAMESPACE`. `LADE_VAULT_HTTP` for http. |
| Passbolt      | `passbolt://DOMAIN/RESOURCE_ID/FIELD`                | Uses the Passbolt CLI.                              |
| AWS Secrets Manager | `awssm://REGION/NAME`                          | Optional `?query=`, `?version=`, `?version_stage=`. String secrets only. `AWS_PROFILE` / default SDK chain. |
| Azure Key Vault | `azuresm://VAULT/NAME`                             | Optional `?query=`. Vault name or `vault.vault.azure.net` (also `.usgovcloudapi.net`, `.azure.cn`). One resolve cannot mix sovereign clouds without `AZURE_ACCESS_TOKEN`. `AZURE_ACCESS_TOKEN` / `LADE_AZURE_TOKEN` or Azure ADC. |
| GCP Secret Manager | `gcpsm://PROJECT/NAME`                          | Optional `?query=` and `?location=` for regional secrets. UTF-8 payloads. `GOOGLE_OAUTH_ACCESS_TOKEN` / `CLOUDSDK_AUTH_ACCESS_TOKEN` / `LADE_GCP_TOKEN` or ADC. |
| age           | `age://CIPHERTEXT` · `?plugin=` · `?identity=`       | Path is the ciphertext. Query last. Same keys as SOPS: `plugin`, `identity`, `identity_file`. |
| SOPS          | `sops://PATH` · `?query=.field` · `?plugin=`         | One decrypt per path + plugin + identity. No plugin: process env. `plugin` remaps named env vars into the SOPS child. |
| File          | `file://PATH?query=.fields[0].field`                 | `?query=` is required. INI, JSON, YAML, and TOML.   |
| Shell command | `sh://gcloud auth print-access-token`                | Also `bash://`, `zsh://`, and `fish://`. Wrap: `fish --no-config`, `zsh -f`, `$BASH_ENV` cleared. |
| Inline value  | `"visible-in-lade-yml"`                              | Use `!` to force a raw value and `!!` to keep a leading `!`. |

Use `lade eval <uri>` to resolve one URI when debugging a provider.
Eval writes an `access` diary row (the URI, not the value). No
`lade.yml` `log` flag.

The release, installer, and `cargo install` put `age-plugin-lade` on
the PATH next to `lade`. Both names are real binaries from the same
crate. age and rage load the plugin when they see an `age1lade1…`
recipient or an `AGE-PLUGIN-LADE-1…` identity. The payload is a Lade
URI. The plugin hydrates it the same way eval does, then lets the age
crate wrap or unwrap with the returned key (X25519, SSH, tagged, and
post-quantum `age1tagpq1` recipients). `age-plugin-lade` plus a URI
prints the identity file.

```bash
age-plugin-lade 'file://./age.json?query=.key' > identity.txt
age -r "$(grep Recipient: identity.txt | awk '{print $3}')" -o secret.age secret.txt
age -d -i identity.txt -o secret.txt secret.age
```

`age://` details:

- Grammar is `scheme://PATH?params`, same as SOPS and file. Path is the ciphertext. Query names how to open it.
- Armored (`-----BEGIN AGE ENCRYPTED FILE-----`) or binary (`age-encryption.org`). Percent-encode the path when it has spaces or newlines.
- Native: `LADE_AGE_KEY` or `LADE_AGE_KEY_FILE`, or `?identity=CI_AGE` / `?identity_file=`.
- Hardware or KMS is a query param, not a path segment: `age://CIPHERTEXT?plugin=yubikey&identity=YUBI_ID`. Requires `age-plugin-yubikey` on PATH at hydrate. `lade lock` does not pin plugins. Values are environment variable names, not secrets.
- One unwrap per `(plugin, identity, ciphertext)`. Plugin groups run one after another. `sh://` stays the exception: the rest is an opaque script.

`sops://` details:

- Path is relative to the `lade.yml` directory, or absolute. `~/` and `$HOME/` expand. Optional `?query=` is a JSON path after decrypt (`--output-type json`).
- No `plugin`: SOPS uses the process env (`SOPS_AGE_KEY`, AWS/GCP/Azure/Vault chain).
- `?plugin=age&identity=CI_AGE` remaps `CI_AGE` to `SOPS_AGE_KEY` for the child. `identity_file` remaps to `SOPS_AGE_KEY_FILE`.
- Other `plugin` values: `pgp` (`homedir` → `GNUPGHOME`), `aws_kms` (`profile`, `region`), `gcp_kms` (`credentials`), `azure_kv` (`token`), `hc_vault` (`token`). Any other valid name is an age plugin (`age-plugin-NAME` on PATH) and uses the age identity slots.
- Recipients stay in the file. The URI names the plugin and the env var names.

`file://` details:

- `?query=` is a JSON path after the file is parsed (`access_json`). Examples:
  `.token`, `.db.password`, `.fields[0].field`, `.section.password` for INI.
- Path is relative to the `lade.yml` directory, or absolute. `~/` and `$HOME/`
  expand to the user home. Spaces in the path must be percent-encoded (`%20`).
- Extension selects the parser: `.json`, `.yaml` / `.yml`, `.toml`, `.ini`.
- A `file://` URI without `?query=` is not loaded as a file. It stays the
  literal string (same Raw fallback as an unknown scheme). Other registered
  schemes fail closed on `add`.

`sh://` / `bash://` / `zsh://` / `fish://` details:

- Everything after `scheme://` is the script. It cannot be empty.
- Wrap argv, also used by `lade inject` and `lade hook`. Always on. There is no
  `lade.yml` or CLI flag to turn it off.

| Shell | Wrap argv |
| ----- | --------- |
| Fish | `fish --no-config -c …` |
| Zsh | `zsh -f -c …` |
| Bash / sh | `bash -c …` with `$BASH_ENV` unset |

`--norc --noprofile` are not used: they do not skip `$BASH_ENV`, and `bash -c`
does not read `.bashrc` or login profiles anyway. Pre-exec (`lade set`) still
evals in the live interactive shell, so the profile stays in play there.
`lade status` prints `inject wrap: skips startup files` and names the file or
`BASH_ENV` when it is present.

- Lade recognizes `$NAME` and `${NAME}` to build the dependency graph, then
  passes those resolved values as environment variables. The script text is
  not rewritten. Quote expansions (`"$user"`) so values stay one argument.
- Output is treated as a secret and masked like other provider-resolved values.

Bindings can compose URIs. `${NAME}`, `$NAME`, and `${.NAME}` pull another
binding in the same rule. YAML `null` or `~` on a key cancels a value inherited
from a parent `lade.yml`. A later matching rule overlays the same key.

### Intermediate bindings

Use a `.NAME` binding when a resolved value only helps construct another
binding. It remains private to the one command invocation, while the public
binding is injected into the requested output:

```yaml
"curl .*api\\.example\\.com.*":
  .API_KEY: op://company/api/key
  Authorization: "Bearer ${API_KEY}"
```

Here `Authorization` is injected; `API_KEY` is not. Private bindings can depend
on other bindings and are included in masking when their resolved values reach
a public value. The end-to-end terminal demo is
[examples/tape/intermediate.exp](examples/tape/intermediate.exp).

### Shell transforms

Same wrap and `$NAME` graph as the `sh://` table. The script is not rewritten.
Example: compose Basic auth without injecting the password.

```yaml
"curl .*api\\.example\\.com.*":
  user: demo-user
  .password: op://company/api/password
  Authorization: 'sh://printf "Basic %s" "$(printf "%s:%s" "${user}" "$password" | base64 | tr -d "\n")"'
```

### Files and disclaimers

Options under `.` configure the matched command itself.

```yaml
"deploy .*":
  .:
    file: secrets.yml
    disclaimer: "This command will use production credentials."
    log: true
  API_TOKEN: op://DOMAIN/VAULT/ITEM/FIELD
```

`when` is `always` (default), `human`, or `agent`. `lade hook` and
`--pretool` are `agent`. `lade set` / `unset` are `human`. Otherwise env
signals (`AI_AGENT`, `CURSOR_AGENT`, `CLAUDECODE`, not `CURSOR_VERSION`)
select `agent`. The same pattern can be a YAML list of these blocks when
`when` differs. `silence` skips that rule's secret progress lines.

```yaml
"^git ":
  - .:
      when: human
    SSH_AUTH_SOCK: sh://launchctl getenv SSH_AUTH_SOCK
  - .:
      when: agent
    SSH_AUTH_SOCK: 'sh://printf %s "$HOME/.ssh/agent.sock"'
```

With hooks, disclaimers cannot prompt for input. Lade withholds access and
prints an approval code; review it, then run `lade approve <code>` or re-run the
command with `LADE_APPROVE=<code>`.

### Per-user values

```yaml
"deploy .*":
  API_TOKEN:
    alice: op://DOMAIN/VAULT/ALICE_TOKEN/FIELD
    ci: vault://DOMAIN/MOUNT/ci-token/value
    .: op://DOMAIN/VAULT/DEFAULT_TOKEN/FIELD
```

```bash
lade user
lade user alice
lade user --reset
```

### Networks

Assign a URI to an environment variable for a dynamic local port, or to a
number for a fixed local port.

```yaml
"psql .*":
  DB_PORT: kubectl://k8s.example.com:6443/prod/default/service/postgres/5432
  1223: ssh://jump.example.com:22/db.internal/5432
```

A numeric key is the local listen port. An env-var key gets an ephemeral local
port unless `local=` sets one. Without `local=`, Lade binds `127.0.0.1`.
Userinfo (`user:pass@`) is rejected. Unknown query keys fail instead of being
ignored. A malformed network URI fails closed. It is not treated as a raw
string.

Supported network providers:

| Provider  | URI                                                                                                   | Query options                                               |
| --------- | ----------------------------------------------------------------------------------------------------- | ----------------------------------------------------------- |
| `kubectl` | `kubectl://<cluster-host>:<cluster-port>/<context-selector>/<namespace>/<kind>/<name>/<remote-port>`  | `local=HOST:PORT`, `pod-running-timeout=<duration>`         |
| `kubefwd` | `kubefwd://<cluster-host>:<cluster-port>/<context-selector>/<namespace>/<kind>/<name>/<service-port>` | `local=HOST:PORT`, `domain=<domain>`, `selector=<selector>` |
| `tsh`     | `tsh://<proxy-host>:<proxy-port>/<kind>/<resource-path>`                                              | `local=HOST:PORT`                                           |
| `ssh`     | `ssh://<jump-host>:<jump-port>/<remote-host>/<remote-port>`                                           | `local=HOST:PORT`                                           |

Query options:

- `local=HOST:PORT`: bind that local endpoint. Both parts are required. On a
  numeric key, `PORT` must match the key. On an env-var key, that port is
  written into the variable. `tsh` app proxy accepts only `127.0.0.1` or
  `localhost`.
- `pod-running-timeout` (`kubectl` only): passed through to
  `kubectl port-forward --pod-running-timeout`.
- `domain` / `selector` (`kubefwd` only): forwarded to `kubefwd`.

For `tsh`, `<kind>` uses Teleport resource nomenclature:

- `app/<app-name>`: Teleport app proxy (for example Grafana).
- `app/<app-name>/<target-port>`: same, with an explicit target port.
- `kube_cluster/<kube-cluster>/<namespace>/<resource-kind>/<name>/<remote-port>`:
  forward a Kubernetes resource through Teleport.

`ssh` jump port defaults to `22` when the authority has no port
(`ssh://jump.example.com/db.internal/5432`).

See [examples/tape/lade.yml](examples/tape/lade.yml) and
[examples/tape/network.txt](examples/tape/network.txt) for more examples.

<details>
<summary>1Password service account tokens</summary>

In CI, `OP_SERVICE_ACCOUNT_TOKEN` is usually injected directly by the platform.
If the token itself lives in another vault, add `1password_service_account` to
the `.` block. Lade resolves that URI first and uses it while resolving
remaining `op://` secrets.

```yaml
"deploy .*":
  .:
    1password_service_account: vault://DOMAIN/MOUNT/KEY/FIELD
  API_TOKEN: op://DOMAIN/VAULT/ITEM/FIELD
```

</details>

## CI and containers

The installer runs non-interactively in CI when `CI=1`, `ASSUME_YES=1`, or stdin
is not a TTY.

```bash
curl -fsSL https://raw.githubusercontent.com/zifeo/lade/main/installer.sh | CI=1 bash
```

### GitHub Actions

```yaml
steps:
  - uses: zifeo/lade@v0.15.3
    with:
      version: "0.15.3"
  - run: lade inject -- terraform apply
    env:
      OP_SERVICE_ACCOUNT_TOKEN: ${{ secrets.OP_SERVICE_ACCOUNT_TOKEN }}
```

### GitLab CI

```yaml
deploy:
  script:
    - curl -fsSL https://raw.githubusercontent.com/zifeo/lade/main/installer.sh | CI=1 VERSION=0.15.3 bash
    - lade inject -- terraform apply
```

### Docker

```dockerfile
COPY --from=ghcr.io/zifeo/lade:0.15.3 /usr/local/bin/lade /usr/local/bin/lade
```

The `ghcr.io/zifeo/lade` image is published for `linux/amd64` and `linux/arm64`
with tags `X.Y.Z`, `X.Y`, and `latest`. Pin an exact `X.Y.Z` for reproducible
builds.

See [docs/](docs/) for internals.

## Development

```bash
eval "$(lade off)"
eval "$(cargo run -- on)"
echo a $A1 $A2 $B1 $B2 $B3 $C1 $C2 $C3
cargo run -- -vvv set echo a
cargo run -- inject echo a
eval "$(cargo run -- off)"
eval "$(lade on)"
```
