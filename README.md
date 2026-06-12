# Lade

![Crates.io](https://img.shields.io/crates/v/lade)

Lade (/leɪd/) is a tool allowing you to automatically load secrets from your
preferred vault into environment variables or files. It limits the exposure of
secrets to the time the command requiring the secrets lives.

<p align="center">
  <img src="./examples/tape/main.gif" alt="Demo" />
</p>

## Getting started

You can download the binary executable from
[releases page](https://github.com/zifeo/lade/releases) on GitHub, make it
executable and add it to your `$PATH` or use the method below to automate those
steps.

```bash
# recommended way
curl -fsSL https://raw.githubusercontent.com/zifeo/lade/main/installer.sh | bash

# or alternative ways via cargo
cargo install lade --locked
cargo install --git https://github.com/zifeo/lade --locked

# upgrade
lade upgrade

# install shell hooks (only required once)
lade install
```

Compatible shells: [Fish](https://fishshell.com),
[Bash](https://www.gnu.org/software/bash/), [Zsh](https://zsh.sourceforge.io)

Compatible vaults: [Infisical](https://infisical.com),
[1Password CLI](https://1password.com/downloads/command-line/),
[Doppler](https://www.doppler.com), [Vault](https://github.com/hashicorp/vault),
[Passbolt](https://www.passbolt.com)

## Features

<table>
<tr>
<td width="50%">

**Shell hooks** - Run `lade install` once. Secrets load automatically around every matching command; `lade off` / `lade on` to pause.

</td>
<td width="50%">

![Shell hooks](./examples/tape/hooks.gif)

</td>
</tr>
<tr>
<td width="50%">

**Secret resolution & command matching** - Load from Infisical, 1Password, Doppler, Vault, Passbolt, the [file loader](#file-loader) (`file://…?query=…`), or inline values. Lade merges every `lade.yml` from the current directory up to the repo root. Each block is a regex on the command you run.

</td>
<td width="50%">

![Secret resolution](./examples/tape/resolution.gif)

</td>
</tr>
<tr>
<td width="50%">

**Manual injection & redaction** - `lade <command>` is the shortcut for one-shot injection when hooks are off or in scripts (explicit form: `lade inject <command>`). Unless `--no-mask` is set, values fetched from loaders are masked in stdout/stderr as `${VAR_NAME:-REDACTED}`. The [raw loader](#raw-loader) values are not (already plaintext in `lade.yml`).

</td>
<td width="50%">

![Manual injection](./examples/tape/inject.gif)

</td>
</tr>
<tr>
<td width="50%">

**Secrets as files** - `file:` under `.` writes JSON/YAML for the command; Lade removes the file when the command exits.

</td>
<td width="50%">

![Secrets as files](./examples/tape/file-output.gif)

</td>
</tr>
<tr>
<td width="50%">

**Per-user secrets** - Map usernames to different values; `lade user` selects who you are (`"."` is the default).

</td>
<td width="50%">

![Per-user secrets](./examples/tape/per-user.gif)

</td>
</tr>
<tr>
<td width="50%">

**Disclaimer** - Optional `disclaimer:` on a rule; type `yes` before secrets load. In hook mode, use `lade approve` to consent.

</td>
<td width="50%">

![Disclaimer](./examples/tape/disclaimer.gif)

</td>
</tr>
<tr>
<td width="50%">

**`lade eval`** - Resolve one URI and print the value (uses the same loaders as `lade.yml`).

</td>
<td width="50%">

![lade eval](./examples/tape/eval.gif)

</td>
</tr>
</table>

## Usage

See [lade.yml](lade.yml) or [examples/tape/lade.yml](examples/tape/lade.yml) for
configuration samples.

### Per-user secrets

```yaml
command regex:
  SAME_SECRET_FOR_EVERYONE: hello_world
  SECRET_FOR_THE_USER:
    alex: alex_secret
    zifeo: zifeo_secret
    .: default_secret
```

```sh
lade user              # show currently set user
lade user tonystark    # set user to tonystark
lade user --reset      # reset, falling back to the OS user
```

### Outputting as files & interactive disclaimer

Both options live under `.` on a rule.

```yaml
command regex:
  .:
    file: secrets.yml
    disclaimer: "This command will use your API token."
  SECRET: op://...
```

When using shell hooks, disclaimers cannot prompt for input. Instead, Lade will withhold secrets and ask you to run `lade approve` to review the disclaimer and execute the command. You can also bypass the check for a single command with `LADE_ACCEPT_DISCLAIMER=1 <command>`.

## Loaders

Most of the vault loaders use their native CLI to operate. This means you must
have them installed locally and your login/credentials must be valid. Lade may
evolve by integrating directly with the corresponding API, but this is left as
future work.

### Infisical loader

```yaml
command regex:
  EXPORTED_ENV_VAR: infisical://DOMAIN/PROJECT_ID/ENV_NAME/SECRET_NAME
```

Frequent domain(s): `app.infisical.com`.

Note: the `/api` is automatically added to the DOMAIN. This source currently
only support a single domain (you cannot be logged into multiple ones).

### 1Password loader

```yaml
command regex:
  EXPORTED_ENV_VAR: op://DOMAIN/VAULT_NAME/SECRET_NAME/FIELD_NAME
```

Frequent domain(s): `my.1password.eu`, `my.1password.com` or `my.1password.ca`.

In CI/CD `OP_SERVICE_ACCOUNT_TOKEN` is typically injected directly by the
platform. For cases where the token itself is stored in another vault, add
`1password_service_account` to the `.` config block. Lade resolves that URI
first - using any loader - and injects the result as `OP_SERVICE_ACCOUNT_TOKEN`
before resolving the remaining `op://` secrets. This enables recursive
cross-vault lookups: the token lives in Vault or Infisical, and the actual
secrets live in 1Password.

Per-user mapping lets each developer or environment use a different source for
the token, or skip it entirely with `null` to fall back on their local `op` session.

```yaml
command regex:
  .:
    # simple: token stored in 1Password itself (requires an active op session)
    1password_service_account: op://DOMAIN/VAULT/ITEM/FIELD
    # or per-user: CI pulls token from Vault, others use their local op session
    # 1password_service_account:
    #   ci: vault://DOMAIN/MOUNT/KEY/FIELD
  EXPORTED_ENV_VAR: op://...
```

### Doppler loader

```yaml
command regex:
  EXPORTED_ENV_VAR: doppler://DOMAIN/PROJECT_NAME/ENV_NAME/SECRET_NAME
```

Frequent domain(s): `api.doppler.com`.

### Vault loader

```yaml
command regex:
  EXPORTED_ENV_VAR: vault://DOMAIN/MOUNT/KEY/FIELD
```

### Passbolt loader

```yaml
command regex:
  EXPORTED_ENV_VAR: passbolt://DOMAIN/RESOURCE_ID/FIELD
```

### File loader

Supports INI, JSON, YAML and TOML files.

```yaml
command regex:
  EXPORTED_ENV_VAR: file://PATH?query=.fields[0].field
```

`PATH` can be relative to the lade directory, start with `~`/`$HOME` or absolute
(not recommended when sharing the project with others as they likely have
different paths).

### Raw loader

```yaml
command regex:
  EXPORTED_ENV_VAR: "value"
```

Escaping a value with the `!` prefix enforces the use of the raw loader and
double `!!` escapes itself.

## LLM Agent Hooks

Lade integrates with agentic tools to automatically inject secrets into agent shell commands.

```bash
lade hook  # reads JSON from stdin, outputs platform-specific response
# example Cursor config:
cat .cursor/hooks.json
```

### Cursor

When `CURSOR_VERSION` is detected. [Docs](https://cursor.com/docs/agent/hooks)

`.cursor/hooks.json`:

```json
{
  "version": 1,
  "hooks": { "preToolUse": [{ "command": "lade hook", "matcher": "Shell" }] }
}
```

### Claude Code

When `CLAUDE_PROJECT_DIR` is detected. [Docs](https://docs.anthropic.com/en/docs/claude-code/hooks)

`.claude/settings.json`:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "lade hook" }]
      }
    ]
  }
}
```

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
