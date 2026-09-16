# Lade

![Crates.io](https://img.shields.io/crates/v/lade)

**Why.** A command needs secrets or a private network. That access
should exist for the process, then be gone.

**What.** One `lade.yaml`. Three families: secret, tunnel, bin. The
same wrap for humans and agents. See which access was used.

**How.** Install, `lade setup`, type the command.

<p align="center">
  <img src="./examples/tape/main.gif" alt="Demo" />
</p>

Lade on [Fish](https://fishshell.com),
[Bash](https://www.gnu.org/software/bash/), or [Zsh](https://zsh.sourceforge.io).
macOS and Linux.

## Getting started

```bash
curl -fsSL https://raw.githubusercontent.com/zifeo/lade/main/installer.sh | bash
```

```bash
cargo install lade --locked
```

An agent should download
[`agent-setup.sh`](https://raw.githubusercontent.com/zifeo/lade/main/agent-setup.sh)
and run it from the git repo. That script installs Lade if needed
and runs `lade setup`. It is the prompt to fetch, not a second
install path to paste next to the human curl.

Then `cd` into the repo and run `lade setup`. Setup prints the
reload line when this shell just got its first wrap.

You do not prefix the command. Pre-exec (or the agent hook) wraps
it. Without a wrap: `lade tofu apply` or `lade inject -- tofu apply`.

`lade on` / `lade off` pause this shell.
`lade hook enable --shell` wraps another profile.

## Patterns

<table>
<tr>
<td width="50%">

**pre-exec.** Run commands normally. Lade injects access only when
the command matches `lade.yaml`.

</td>
<td width="50%">

![pre-exec](./examples/tape/hooks.gif)

</td>
</tr>
<tr>
<td width="50%">

**Provider resolution.** Match commands and load values from vaults,
files, or inline config only when needed.

</td>
<td width="50%">

![Provider resolution](./examples/tape/resolution.gif)

</td>
</tr>
<tr>
<td width="50%">

**Private networks.** Open a local forward only while the command
runs, then close it automatically.

</td>
<td width="50%">

![Private network](./examples/tape/network.gif)

</td>
</tr>
<tr>
<td width="50%">

**Secrets as files.** Write temporary config files for commands that
expect credentials on disk.

</td>
<td width="50%">

![Secrets as files](./examples/tape/file-output.gif)

</td>
</tr>
<tr>
<td width="50%">

**Manual injection.** Use `lade <command>` in scripts, CI, or shells
without hooks. The explicit form is `lade inject <command>`.

</td>
<td width="50%">

![Manual injection](./examples/tape/inject.gif)

</td>
</tr>
</table>

More: [examples/tape/](examples/tape/). Re-record GIFs after
`examples/tape/render.sh` when the setup copy changes.

## Config

A `lade.yaml` (or `lade.yml`) at the folder that owns the command.
Lade walks from the current directory up to `$HOME`. The nearest
file wins on a key. The parent is the default.

The first line may be a YAML comment with the required Lade
range. A comment cannot steal a command regex. A rule for `#`
is a quoted key (`"#"` or `"\\#"`). An empty key is also a regex,
not a version (`"": ">0.18,<=0.20"` does not pin Lade). Edit `#:`
by hand when the repo needs a Lade that is new enough. There is
no pin command. `lade upgrade` still runs if this binary is too
old.

```yaml
#: >=0.18.0

"^tofu":
  TF_VAR_api_key: op://DOMAIN/VAULT/ITEM/FIELD
```

```bash
tofu apply
```

## Families

| Family | What |
| --- | --- |
| `secret` | A value for the command (`op://`, `file://`, raw, …). Lands in the process env |
| `tunnel` | A local forward for the process (`kubectl://`, `tsh://`, …) |
| `bin` | A CLI pinned for that command |

`lade add secret` / `lade add tunnel` / `lade add bin` write the
nearest yaml, then run `lade setup`. `env` is an alias of `secret`.

### Secret

A vault, a file, a shell snippet, or a raw string. Provider-resolved
values are masked unless `--no-mask`. Raw is **not** a vault secret.
It is a value you put in the yaml. The wizard warns. It still lives
in this family because it becomes an env var.

```yaml
"^psql":
  DB_USER: op://my.1password.com/eng/postgres/username
  DATABASE_URL: postgres://${DB_USER}@127.0.0.1:${DB_PORT}/app
```

| Provider | URI | Notes |
| --- | --- | --- |
| 1Password | `op://DOMAIN/VAULT/ITEM/FIELD` | Optional section. 1Password CLI. |
| Infisical | `infisical://DOMAIN/PROJECT_ID/ENV_NAME/SECRET_NAME` | Nested folders. Infisical CLI. |
| Doppler | `doppler://DOMAIN/PROJECT_NAME/ENV_NAME/SECRET_NAME` | Doppler CLI. |
| Vault | `vault://DOMAIN/MOUNT/KEY/FIELD` | KV v2. `VAULT_TOKEN` or `~/.vault-token`. |
| Passbolt | `passbolt://DOMAIN/RESOURCE_ID/FIELD` | Passbolt CLI. |
| Bitwarden | `bw://ITEM/FIELD` | `BW_SESSION` after `bw unlock`. |
| AWS Secrets Manager | `awssm://REGION/NAME` | Optional `?query=`, `?version=`. |
| Azure Key Vault | `azurekv://VAULT/NAME` | Optional `?query=`. |
| GCP Secret Manager | `gcpsm://PROJECT/NAME` | Optional `?query=`, `?location=`. |
| age | `age://CIPHERTEXT` | `?plugin=`, `?identity=`. |
| SOPS | `sops://PATH?query=.field` | `?plugin=` remaps env into the SOPS child. |
| File | `file://PATH?query=.fields[0].field` | `?query=` required. INI, JSON, YAML, TOML. |
| Shell | `sh://gcloud auth print-access-token` | Also `bash://`, `zsh://`, `fish://`. |
| Raw | `"visible-in-the-yaml"` | Not a secret. `!` forces raw, `!!` keeps a leading `!`. |

`lade eval <uri>` resolves one URI. Authenticate the provider CLI
first. Lade does not pick the login command. See that product's
docs, then retry.

`.file` on the rule writes a temp JSON/YAML for the command, then
deletes it. That is output, not the file provider.

```yaml
"deploy .*":
  .:
    file: secrets.yml
  API_TOKEN: op://DOMAIN/VAULT/ITEM/FIELD
```

### Tunnel

A local forward for this process. An env-var key gets an ephemeral
local port unless `local=` sets one. A numeric key is a fixed port.

```yaml
"^psql":
  DB_PORT: kubectl://k8s.example.com:6443/prod/default/service/postgres/5432
  1223: ssh://jump.example.com:22/db.internal/5432
```

| Provider | URI | Query |
| --- | --- | --- |
| `kubectl` | `kubectl://<host>:<port>/<context>/<ns>/<kind>/<name>/<remote-port>` | `local=HOST:PORT`, `pod-running-timeout=` |
| `kubefwd` | `kubefwd://<host>:<port>/<context>/<ns>/<kind>/<name>/<service-port>` | `local=`, `domain=`, `selector=` |
| `tsh` | `tsh://<proxy>:<port>/<kind>/<resource-path>` | `local=` |
| `ssh` | `ssh://<jump>:<port>/<remote-host>/<remote-port>` | `local=` |

### Bin

A CLI for the matched command. The key is the argv0 you type.
If the binary is `tofu1.8`, the key is `tofu1.8`. Lade asks the
tool manager to pin it, then puts that install first on PATH for
this process.

```yaml
^tofu:
  tofu1.8: mise://aqua/opentofu/opentofu@1.8.2
  TF_VAR_FOO: op://DOMAIN/VAULT/ITEM/FIELD
```

Today the URI scheme is `mise://`. That is how the current
manager is addressed. A Homebrew binary on PATH is not the pin.
The lock next to this yaml is what the next command must match.

Optional `?setup=` / `?teardown=` run on `lade setup` /
`lade teardown` in this repo.

```yaml
.:
  dcg: mise://github:Dicklesworthstone/destructive_command_guard@0.6.6?setup=install&teardown=uninstall
```

| Scheme | URI | What |
| --- | --- | --- |
| `mise` | `mise://<backend>/<package>@<version>` | A CLI pin |
| `apm` | `apm://<owner>/<repo>` | An APM package CLI plus a ref |
| `skills` | `skills://<owner>/<repo>` | A skills package CLI plus a ref |

## Hierarchy

Walk cwd → `$HOME`. Later file overlays the same key. Child wins.
Parent is the default. A command in `app/` uses `app/lade.yaml`
over the repo root. `~/lade.yaml` is a parent like any other.

YAML `~` on a key cancels a parent value. `${NAME}` composes
bindings in the same rule. `.NAME` is intermediate: used to
build another value, not exported.

Same directory must not have both `lade.yaml` and `lade.yml`.

## Observability

Opt-in. `log: true` on a rule records that the command ran, what
matched, which bin resolved. Values are never stored. The row is
an audit snapshot at write time.

```yaml
.:
  .:
    log: true
```

```bash
lade log
lade usage
lade status
```

Queries stay on this git root. `--all` / `--global` reads wider.
Details: [docs/observability.md](docs/observability.md).

## Humans and agents

Same yaml. Same resolve. The agent types the command. There is
no Lade skill. `lade setup` writes the pre-tool hook. Enable one
harness by hand with `lade hook enable --harness cursor`.

## When, users, approval

`when` is `always` (default), `human`, or `agent`.

```yaml
"^git ":
  - .:
      when: human
    SSH_AUTH_SOCK: sh://launchctl getenv SSH_AUTH_SOCK
  - .:
      when: agent
    SSH_AUTH_SOCK: 'sh://printf %s "$HOME/.ssh/agent.sock"'
```

```yaml
"deploy .*":
  API_TOKEN:
    alice: op://DOMAIN/VAULT/ALICE_TOKEN/FIELD
    .: op://DOMAIN/VAULT/DEFAULT_TOKEN/FIELD
```

```bash
lade user alice
```

A `disclaimer` withholds access. Review, then `lade approve <code>`.

## CI build

Build time only. Same yaml. Not production runtime.

```bash
curl -fsSL https://raw.githubusercontent.com/zifeo/lade/main/installer.sh | CI=1 bash
lade setup
lade inject -- tofu apply
```

GitHub Action: `zifeo/lade`. Image: `ghcr.io/zifeo/lade`.

## More

- Intermediate bindings, `sh://` wrap, age/SOPS query params:
  [docs/architecture.md](docs/architecture.md)
- Diary flags: [docs/observability.md](docs/observability.md)
- MCP one-shot (`lade mcp`): still supported, not the product
- `1password_service_account` on `.` for CI `op://`

## Development

```bash
eval "$(lade off)"
eval "$(cargo run -- on)"
cargo test --workspace --locked
eval "$(lade on)"
```
