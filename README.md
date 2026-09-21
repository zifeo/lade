# Lade

![Crates.io](https://img.shields.io/crates/v/lade)

Temporary access to secrets and private networks for one command,
then gone. Same wrap for humans and harnesses. See which access was used.

A command needs secrets or a private network. That access should
exist for the process, then disappear. One `lade.yaml`. Three
families: secret, tunnel, package. Install, `lade setup`, type
the command.

<p align="center">
  <img src="./examples/tape/main.gif" alt="Demo" />
</p>

Lade on [Fish](https://fishshell.com),
[Bash](https://www.gnu.org/software/bash/), or [Zsh](https://zsh.sourceforge.io).
macOS and Linux. [Cursor](https://cursor.com),
[Claude Code](https://code.claude.com),
[Codex](https://developers.openai.com/codex), and
[OpenCode](https://opencode.ai).

## Getting started

```bash
curl -fsSL https://raw.githubusercontent.com/zifeo/lade/main/installer.sh | bash
cd your-repo
lade setup
```

Setup installs a **shell hook** (pre-exec) once, in your profile, so
every command you type can match. The rest stays in the repo: pre-tool
hooks for Cursor, Claude Code, Codex, and OpenCode harnesses, and the
lock for pinned packages. The wrap is yours. The access is this repo's.

Humans use the shell hook (pre-exec). Harnesses use pre-tool hooks in
the repo. Same `lade.yaml`, same resolve.

You do not prefix the command. Without a wrap: `lade -- tofu apply`.

`lade on` / `lade off` pause pre-exec in this shell.
`lade hook enable --shell` installs pre-exec in this shell's profile.
`lade teardown` removes this repo's pre-tool hooks and runs
`?teardown=` commands. The shell hook stays.

Or from GitHub:

```bash
cargo install --git https://github.com/zifeo/lade --locked
```

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

**One command.** Use `lade -- tofu apply` in scripts, CI, or shells
without hooks.

</td>
<td width="50%">

![One command](./examples/tape/inject.gif)

</td>
</tr>
<tr>
<td width="50%">

**Human approval.** A `disclaimer` withholds access until you
review it and run `lade approve <code>`.

</td>
<td width="50%">

![Human approval](./examples/tape/disclaimer.gif)

</td>
</tr>
<tr>
<td width="50%">

**Intermediate bindings.** `.NAME` builds another value in the
same rule and is not exported.

</td>
<td width="50%">

![Intermediate bindings](./examples/tape/intermediate.gif)

</td>
</tr>
</table>

More: [examples/tape/](examples/tape/).

## Config

A `lade.yaml` (or `lade.yml`) at the folder that owns the command.
Lade walks from the current directory up to `$HOME`. The nearest
file wins on a key. The parent is the default.

The file may pin the required Lade range with an empty YAML key
(`: >=0.18.0`). An empty command regex is not a rule. Use `.` to
match every command. Edit `:` by hand when the repo needs a Lade
that is new enough. There is no pin command. `lade upgrade` still
runs if this binary is too old.

```yaml
: >=0.18.0

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
| `package` | A pinned CLI or setup package (`mise://`, `apm://`, `skills://`) |

`lade add secret` / `lade add tunnel` / `lade add package` write the
nearest yaml, then run `lade setup`. `lade remove` drops a binding and
does not run setup.

Family aliases (CLI only): `env` → `secret`; `net` / `network` / `fwd`
→ `tunnel`; `pkg` / `cli` / `tool` / `apm` / `skill` → `package`.

### Secret

A vault, a file, a shell snippet, or a raw string. Provider-resolved
values are masked unless `--no-mask`. Raw is **not** a vault secret.
It is a value you put in the yaml. `lade add` warns once. A wrap
does not. It still lives in this family because it becomes an env
var.

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
| Vault | `vault://DOMAIN/MOUNT/KEY/FIELD` | KV v2. `VAULT_TOKEN` or `~/.vault-token`. Setup locks the Vault CLI. |
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
| `tsh` | `tsh://<proxy>:<port>/app/<app-name>[/<target-port>]` or `.../kube_cluster/<cluster>/<ns>/<kind>/<name>/<remote-port>` | `local=` |
| `ssh` | `ssh://<jump>:<port>/<remote-host>/<remote-port>` | `local=` |

### Package

A CLI for the matched command, or a setup-only package. The key
is the argv0 you type. If the binary is `tofu1.8`, the key is
`tofu1.8`. Lade asks the tool manager to pin it, then puts that
install first on PATH for this process.

```yaml
^tofu:
  tofu1.8: mise://aqua/opentofu/opentofu@1.8.2
  TF_VAR_FOO: op://DOMAIN/VAULT/ITEM/FIELD
```

Today the URI scheme is `mise://`. That is how the current
manager is addressed. A Homebrew binary on PATH is not the pin.
`lade setup` walks toward `$HOME` once. A committed `mise.toml`
or `mise.lock` in the repo is the plane: setup extends both.
`$HOME` itself counts only for `~/lade.yaml`. Otherwise one
`lade.lock` at the git root. A git repo in `$HOME` is not that
root. Missing slots are resolved once and written.

| Command | Lock | Yaml |
| --- | --- | --- |
| `lade setup` | Install this version | Exact pins stay. A range may heal from `mise.toml` |
| `lade setup --unlock` | Ignore it this once, then rewrite | Resolve and install what yaml says now |
| `lade update` | Rewrite after a new resolve | Exact pins stay. Implied and ranged pins move to the latest match |
| `lade upgrade` | Untouched | Untouched. This is the Lade binary |

`lade teardown` is this repo only: pre-tool hooks and `?teardown=`
on setup packages. It does not remove the shell wrap.

Optional `?setup=` / `?teardown=` run on `lade setup` /
`lade teardown` in this repo.

```yaml
.:
  dcg: mise://github:Dicklesworthstone/destructive_command_guard@0.6.6?setup=install&teardown=uninstall
```

| Scheme | URI | What |
| --- | --- | --- |
| `mise` | `mise://<backend>/<package>@<version>` | A CLI pin. `lade add package` writes this. |
| `apm` | `apm://<registry-path>` | Setup-rule package only. Not a command pin. |
| `skills` | `skills://<registry-path>` | Setup-rule package only. Not a command pin. |

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
matched, which package resolved. Values are never stored. The row is
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

## Shell and harnesses

Same yaml. Same resolve. Humans type commands in a wrapped shell
(pre-exec). Harnesses run commands through pre-tool hooks.
`lade setup` writes both when needed. Limit harness installs with
`lade setup --harness cursor` (repeat for several). If pre-exec is
missing later, `lade status` and setup point at
`lade hook enable --shell`. Harness hooks:
`lade hook enable --harness <slug>` (recovery; hidden from default
`--help`, see [docs/protocol.md](docs/protocol.md)).

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

## Commands

User-facing verbs. Internal injection mechanics (`set`, `unset`, bare
`hook` on stdin) stay out of `lade --help`; see
[docs/protocol.md](docs/protocol.md).

| Command | Role |
| --- | --- |
| `lade setup` | This repo: packages, lock, first-time pre-exec, pre-tool. `--unlock`, `--harness`. |
| `lade update` | Re-resolve implied and ranged pins; rewrite lock. |
| `lade teardown` | Remove repo pre-tool hooks; run `?teardown=`. |
| `lade add` / `lade remove` | Edit nearest `lade.yaml`. `add` runs setup. Flags: `--rule`, `--key`, `--uri`. |
| `lade on` / `lade off` | Toggle pre-exec snippets for this shell. |
| `lade user` | Set per-user secret map key. `--reset` for OS default. |
| `lade approve <code>` | After a `disclaimer`, or prefix `LADE_APPROVE=<code>`. Exit `3` when withheld. |
| `lade eval <uri>` | Resolve one secret URI to stdout. |
| `lade bench` | Time parse, match, hydrate. `--json`, `--timeout`. |
| `lade upgrade` | Install newer `lade` and `age-plugin-lade`. `--version`, `-y`. |
| `lade status` | Version, hooks, mise, providers. `--json`, `--all`. |
| `lade log` / `lade usage` | Diary queries. See [docs/observability.md](docs/observability.md). |
| `lade mcp` | MCP bridge (below). |
| `lade -- <cmd>` | One-shot wrap (same as documented inject path). |

`silence: true` on a rule skips secret progress lines for that rule.
Provider URIs can **imply** a locked CLI on setup (`op://` → `op`,
`vault://` → `vault`, …) even when the yaml has no `mise://` pin.
`lade status` reports min CLI versions and `sdk` vs `cli` transport.

## MCP

When a harness needs MCP rather than shell rewrite:

```bash
# stdio server
lade mcp -- npx -y @modelcontextprotocol/server-everything

# remote Streamable HTTP
lade mcp https://example.com/mcp
```

Bindings become upstream headers (HTTP) or env for the child (stdio).
Same rule matching, disclaimers, and diary as `lade -- <cmd>`. Details:
[docs/architecture.md](docs/architecture.md).

## CI build

Build time only. Same yaml. Not production runtime.

```bash
curl -fsSL https://raw.githubusercontent.com/zifeo/lade/main/installer.sh | CI=1 bash
lade setup
```

Installer env: `OUT_DIR`, `VERSION`, `PLATFORM`, `ASSUME_YES`, `CI`,
`DOWNLOADER` ([docs/env.md](docs/env.md)).

GitHub Action `zifeo/lade` (`setup-lade`): inputs `version` (default
`latest`), `out-dir` (default `${{ github.workspace }}/.lade-bin`).
Installs `lade` and `age-plugin-lade` with cache.

Image: `ghcr.io/zifeo/lade` (`linux` musl, `ENTRYPOINT` is `lade`).

## age plugin

`age-plugin-lade` lets [age](https://github.com/FiloSottile/age)
hydrate a secret through Lade. The plugin execs `lade eval`. Keep
`lade` next to it or on `PATH`.

```bash
cargo install age-plugin-lade --locked
```

From this repo the plugin is `crates/age-plugin-lade`:

```bash
cargo install --path crates/age-plugin-lade --locked
```

## More

A command can build one value from another (`${NAME}`), run a
shell snippet (`sh://`), or pull a field from age or SOPS. Blueprint:
[docs/architecture.md](docs/architecture.md). T tickets and internal
env: [docs/protocol.md](docs/protocol.md),
[docs/env.md](docs/env.md). Provider examples:
[examples/providers/lade.yml](examples/providers/lade.yml).

`lade log` and `lade usage` flags:
[docs/observability.md](docs/observability.md).

CI that talks to 1Password without a person: set
`1password_service_account` on `.` (per-user map supported; see
examples).

## Development

```bash
eval "$(lade off)"
eval "$(cargo run -- on)"
cargo test --workspace --locked
eval "$(cargo run -- off)"
eval "$(lade on)"
```
