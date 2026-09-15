# Lade

![Crates.io](https://img.shields.io/crates/v/lade)

Temporary access to secrets and private networks for one command,
then gone. Same wrap for humans and agents. See which access was used.

<p align="center">
  <img src="./examples/tape/main.gif" alt="Demo" />
</p>

Lade (/leɪd/) on [Fish](https://fishshell.com),
[Bash](https://www.gnu.org/software/bash/), or [Zsh](https://zsh.sourceforge.io).
macOS and Linux.

You clone a repo, run `lade setup`, type the command. The matching
rule hydrates what that process needs. When it exits, that access is
gone. Humans, agents, and CI **build** use the same `lade.yaml`.
CI **runtime** is out of scope.

Three families on a rule: **secret**, **tunnel**, **bin**. A tool
manager (today [mise](https://mise.jdx.dev); it could be nix) pins
CLIs. That name is an implementation detail.

## Getting started

```bash
curl -fsSL https://raw.githubusercontent.com/zifeo/lade/main/installer.sh | bash
cd your-repo
lade setup
```

`lade setup` is **this repo**: locks, agent hooks, setup commands.
The first time this shell has no Lade wrap, setup also writes
pre-exec into this profile, then tells you to reload:

```bash
source ~/.zshrc    # bash: source ~/.bashrc
# or open a new terminal
```

Other shells on this machine are listed, not written.
`lade hook enable --shell` wraps another profile.
`lade on` / `lade off` pause this shell.

An agent, from a git repo:

```bash
curl -fsSL https://raw.githubusercontent.com/zifeo/lade/main/agent-setup.sh | bash
```

```bash
cargo install lade --locked
lade upgrade
```

Then a `lade.yaml` (or `lade.yml`) at the folder that owns the
command. Lade walks from the current directory up to `$HOME`.
The nearest file wins on a key. The parent is the default.

The first line may be a YAML comment with the required Lade
range. A comment cannot steal a command regex. A rule for `#`
is a quoted key (`"#"` or `"\\#"`). Edit it by hand when the
repo needs a Lade that is new enough. There is no pin command.
`lade upgrade` still runs if this binary is too old.

```yaml
#: >=0.18.0
"^tofu":
  TF_VAR_api_key: op://DOMAIN/VAULT/ITEM/FIELD
```

```bash
tofu apply
```

You do not prefix the command. Pre-exec (or the agent hook) wraps
it. Without a wrap: `lade tofu apply` or `lade inject -- tofu apply`.

## Families

| Family | Spoken | What |
| --- | --- | --- |
| `secret` | secret | A value for the command (`op://`, `file://`, raw, …). Lands in the process env |
| `tunnel` | tunnel | A local forward for the process (`kubectl://`, `tsh://`, …) |
| `bin` | binary | A CLI pinned for that command |

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

A CLI for the matched command. The key is the name you type
(`tofu`, not `tofu1.8`). Lade asks the tool manager to pin it,
then puts that install first on PATH for this process.

```yaml
^tofu:
  tofu: mise://aqua/opentofu/opentofu@1.8.2
  TF_VAR_FOO: op://DOMAIN/VAULT/ITEM/FIELD
```

Today the URI scheme is `mise://…`. That is how the current
manager is addressed. A Homebrew binary on PATH is not the pin.
The lock next to this yaml is what the next command must match.

Optional `?setup=` / `?teardown=` run on `lade setup` /
`lade teardown` in this repo.

```yaml
.:
  dcg: mise://github:Dicklesworthstone/destructive_command_guard@0.6.6?setup=install&teardown=uninstall
```

`apm://` and `skills://` are bins too: a package CLI plus a ref.

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
no Lade skill. The hook rewrites it.

```bash
lade hook enable --harness cursor
```

`--scope user` is leftover home hooks. `lade setup` never writes
those.

<details>
<summary>Cursor, Claude, Codex, OpenCode hook files</summary>

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

Claude: `lade hook --harness claude` under `PreToolUse` / `Bash`.
Codex: same shape in `.codex/hooks.json`. Trust the command in
`/hooks`. OpenCode: plugin that runs `lade hook --harness opencode`.

</details>

Without a pre-tool hook, put in `AGENTS.md`: prefix matching
commands with `lade`.

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

## Patterns

<table>
<tr>
<td width="50%">

**pre-exec.** You type the command in this shell.

</td>
<td width="50%">

![pre-exec](./examples/tape/hooks.gif)

</td>
</tr>
<tr>
<td width="50%">

**Secret.** Only the matching rule’s URIs load.

</td>
<td width="50%">

![Provider resolution](./examples/tape/resolution.gif)

</td>
</tr>
<tr>
<td width="50%">

**Tunnel.** Local forward for the process.

</td>
<td width="50%">

![Private network](./examples/tape/network.gif)

</td>
</tr>
<tr>
<td width="50%">

**`.file` output.** Temp JSON/YAML, then deleted.

</td>
<td width="50%">

![Secrets as files](./examples/tape/file-output.gif)

</td>
</tr>
<tr>
<td width="50%">

**Prefix.** No wrap? `lade tofu apply`.

</td>
<td width="50%">

![Manual injection](./examples/tape/inject.gif)

</td>
</tr>
</table>

More: [examples/tape/](examples/tape/). Re-record GIFs after
`examples/tape/render.sh` when the setup copy changes.

## More

- Intermediate bindings, `sh://` wrap, age/SOPS query params:
  [docs/architecture.md](docs/architecture.md)
- Diary flags: [docs/observability.md](docs/observability.md)
- MCP one-shot (`lade mcp`): still supported, not the product
- `1password_service_account` on `.` for CI `op://`

## Coming from an older Lade

New clone: `lade setup` prints the wrap, hooks, and pins this
repo needs. `lade status` is the same report later. The
changelog lists the break.

Already on Lade: `lade status` after upgrade. A daily GitHub
check can also say a newer tag exists. Generated project hooks
now emit `--harness`. Older `--agent` lines still parse. A
`lade.yaml` that starts with `#: >=0.18.0` refuses an older
binary and points at `lade upgrade`.

What changed that you can see:

- Frontend file is `lade.yaml`. Both `.yaml` and `.yml` in one
  directory is an error.
- Required Lade version is the first-line comment `#:`, not a
  YAML document string and not a `version:` key.
- Spoken hook flag is `--harness` (`claude`, `cursor`, `codex`,
  `opencode`). `--agent` stays as a hidden alias.
- `lade setup` / `lade teardown` are this git repo. The shell
  wrap is written once per profile.
- Secret, tunnel, and bin are the three families on a rule.
- A `mise://` pin is locked. Homebrew on PATH is not a
  substitute. `ssh://` uses OpenSSH on this machine.

## Development

```bash
eval "$(lade off)"
eval "$(cargo run -- on)"
cargo test --workspace --locked
eval "$(lade on)"
```
