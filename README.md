# Lade

![Crates.io](https://img.shields.io/crates/v/lade)

Give shell commands and AI agents temporary access to secrets, files, and
private networks, then clean everything up automatically.

<p align="center">
  <img src="./examples/tape/main.gif" alt="Demo" />
</p>

Lade (/leɪd/) matches the command you run, loads only what it needs, masks
provider-resolved secrets from command output, and removes command-scoped files
and network forwards when the process exits.

## Why Lade?

Modern commands need short-lived access: a deploy needs tokens, a migration needs
a private database, an AI agent needs to run a tool without seeing the secrets
behind it. Lade keeps that access scoped to the command instead of your whole
shell session, CI job, or model context.

- Load secrets from [1Password CLI](https://1password.com/downloads/command-line/),
  [Infisical](https://infisical.com), [Doppler](https://www.doppler.com),
  [Vault](https://github.com/hashicorp/vault),
  [Passbolt](https://www.passbolt.com), local files, shell commands, or inline
  values.
- Write temporary JSON/YAML files for tools that expect credentials on disk.
- Open private network access through `kubectl`, `kubefwd`, Teleport `tsh`, or
  SSH only while the command runs.
- Redact provider-resolved secrets from stdout and stderr.
- Work from shells, CI, Cursor, Claude Code, Codex, and OpenCode.

Compatible shells: [Fish](https://fishshell.com),
[Bash](https://www.gnu.org/software/bash/), [Zsh](https://zsh.sourceforge.io).
Lade targets Unix systems: macOS and Linux.

## Getting started

```bash
curl -fsSL https://raw.githubusercontent.com/zifeo/lade/main/installer.sh | bash
lade install
```

`lade install` adds pre-exec for this shell (not every shell it finds) and
pre-tool for detected agents (hook and skill together). After that, matching
commands are wrapped automatically. Pause and resume pre-exec with `lade off`
and `lade on`.

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

Preexec shell hooks are the recommended path because you keep typing normal commands.
When they are unavailable, prefix the command with `lade` for one-shot
injection. The explicit form is `lade inject <command>`.

```bash
lade terraform apply
lade inject -- terraform apply
```

The wrap skips the user profile. Same argv as `sh://`.

## Local command diary

Recording is off until a matching `lade.yml` rule sets `log: true`. Absent
`log` is not a vote. Last explicit `log` on matching rules wins
(parent then child). `log: false` punches a hole for that command
only. A no-match `seen` uses the last explicit `log` on the loaded
walk. A rule may be log-only, with no secrets. Details:
[docs/log.md](docs/log.md).

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

Lade writes one row per opted-in command to a local SQLite file next to the
rest of its user data: `ProjectDirs::from("com", "zifeo", "lade")`,
`data_local_dir()/events.db`. `config.json` uses the same library and
qualifier, on `config_local_dir()`. Vault addresses (`op://…`) are stored.
Secret values are never stored.

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

The default window is the last 90 days. `--since` and `--until` are
durations back from now (`Ns | Nm | Nh | Nd | Nw | Nmonth`, `m` is
minutes). `--limit` is an extra row cap. A bare `--limit 20` drops the
90-day default and returns the newest 20 rows. `--group command`
aggregates the diary by command text, most frequent first. Nothing
prunes by itself. `lade log --help` prints the database path.
`lade status` reports the path, event count, and a human size.

`lade log` and `lade usage` both stay in the current git root when there
is one, including git worktrees (`.git` file). `--all` reads the whole
diary. `--path` scopes to the git root of that directory. `lade log` is
the diary of typed commands. `lade usage` is Lade usage in this tree:
matched `lade.yml` rules only, most frequent first, with the file path
and `env` / `file` / `tunnel` tags. Catch-all `.` rules and unused
rules are omitted. `lade.yml` walk stops at `$HOME`.

## Common patterns

<table>
<tr>
<td width="50%">

**preexec shell hooks** - Run commands normally. Lade injects access only when the
command matches `lade.yml`.

</td>
<td width="50%">

![preexec shell hooks](./examples/tape/hooks.gif)

</td>
</tr>
<tr>
<td width="50%">

**Provider resolution** - Match commands and load values from vaults, files, or
inline config only when needed.

</td>
<td width="50%">

![Provider resolution](./examples/tape/resolution.gif)

</td>
</tr>
<tr>
<td width="50%">

**Manual injection** - Use `lade <command>` in scripts, CI, or shells without
hooks. The explicit form is `lade inject <command>`.

</td>
<td width="50%">

![Manual injection](./examples/tape/inject.gif)

</td>
</tr>
<tr>
<td width="50%">

**Private networks** - Open a local forward only while the command runs, then
close it automatically.

</td>
<td width="50%">

![Private network](./examples/tape/network.gif)

</td>
</tr>
<tr>
<td width="50%">

**Secrets as files** - Write temporary config files for commands that expect
credentials on disk.

</td>
<td width="50%">

![Secrets as files](./examples/tape/file-output.gif)

</td>
</tr>
<tr>
<td width="50%">

**Per-user values** - Keep one shared `lade.yml` while developers, CI, and
environments resolve different values.

</td>
<td width="50%">

![Per-user secrets](./examples/tape/per-user.gif)

</td>
</tr>
<tr>
<td width="50%">

**Human approval** - Add a disclaimer before sensitive commands. Hooks withhold
access until the approval code is used.

</td>
<td width="50%">

![Disclaimer](./examples/tape/disclaimer.gif)

</td>
</tr>
<tr>
<td width="50%">

**Shell command provider** - Use stdout from a local command as a secret value.

</td>
<td width="50%">

![Shell command provider](./examples/tape/shell.gif)

</td>
</tr>
<tr>
<td width="50%">

**Intermediate bindings** - Compose a public value from a private binding
without injecting the private value itself.

</td>
<td width="50%">

![Intermediate bindings](./examples/tape/intermediate.gif)

</td>
</tr>
</table>

## AI agents

AI coding agents often need to run commands that require secrets, private
network access, or both. Lade lets the command access what it needs without
putting secret values in the model context or chat transcript.

### Recommended usage: preTool hooks

Cursor, Claude Code, Codex, and OpenCode can call `lade hook` before shell
commands. When an agent runs a matching command, Lade rewrites it through
`lade inject`, resolves the configured access, and redacts provider-resolved
secret values from stdout and stderr.

The agent keeps using normal commands. Lade handles the sensitive part.

`lade install` does two jobs. **pre-exec** wraps commands you type. It
detects Fish, Bash, and Zsh, then installs this shell only. **pre-tool**
wraps commands agents run. The hook and the skill are always written
together. In a git repo it defaults to this repo. Outside git it
defaults to this machine. It then confirms the detected agents.
`--cursor` and friends skip that confirm. If the repo plane is written
while machine hooks already exist, it warns: both hook processes still
run. `lade uninstall` removes this shell's pre-exec and the same
default pre-tool plane. `lade status` prints `run \`lade install\`` on
drift.

The preferred hook for a repo is project scope, so every clone gets the same
guard. That is the default:

```bash
lade hook install --harness cursor
lade hook install --scope user --harness cursor
```

`--scope user` is this machine (`CODEX_HOME` for Codex). `lade install`
in a git repo writes the project plane instead. `lade hook uninstall`
takes the same flags.
`--harness` is required in the files. Auto-detect is a safety net.

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

### Agents without preTool hooks

For agents without shell hooks, add a short instruction to `AGENTS.md`:

```text
When a command needs access defined in lade.yml, prefix it with lade.
Example: lade terraform apply
```

Transparent hooks are preferred because the agent does not need to guess which
commands match `lade.yml`.

## MCP

Desktop MCP clients normally launch a server without the shell environment
where a secret manager is available. Putting long-lived credentials directly in
the client configuration is inconvenient and exposes them to every process
launched from that app. `lade mcp` makes the client launch Lade instead: it
resolves the matching access only for that MCP connection, then exits and
cleans up when the connection closes.

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

Keys prefixed with `.` are intermediate variables. They can be referenced with
`$NAME`, `${NAME}`, or `${.NAME}`, participate in secret masking, and are never emitted to the child
environment, temporary file, or HTTP headers. `.` on its own remains the rule
configuration block. A public key and `.KEY` cannot be declared together.

Intermediate variables belong to binding resolution, not MCP: they work with
shell hooks, `lade inject`, file output, and MCP. For example, only the
composed value is injected:

```yaml
"deploy .*":
  .TOKEN: op://company/production/deploy/token
  DEPLOY_AUTHORIZATION: "Bearer ${TOKEN}"
```

```bash
lade inject -- deploy production
```

The child receives `DEPLOY_AUTHORIZATION`, never `TOKEN`.

To troubleshoot an MCP connection, add `-v` before `mcp` in the client
configuration arguments. Lade writes action-only traces to stderr, such as
`mcp http -> tools/call` and `mcp http <- 200 (42 ms)`. It never logs headers,
JSON-RPC parameters, request bodies, or resolved values. Use `-vv` for debug
logs; `LADE_LOG` overrides the command-line verbosity.

## Configuration reference

Lade has two provider families used from the same `lade.yml` rule:

- Secret providers resolve values into environment variables or temporary files.
- Network providers create command-scoped connectivity and clean up
  automatically.

### Secrets

```yaml
"terraform .*":
  TF_VAR_api_key: op://DOMAIN/VAULT/ITEM/FIELD
```

Most secret providers use their native CLI. Ensure the required binaries are
installed and authenticated before running commands. Provider-resolved values
are masked from command output unless `--no-mask` is set. Inline values are not
masked because they are already visible in `lade.yml`.

Supported secret providers:

| Provider      | URI                                                  | Notes                                               |
| ------------- | ---------------------------------------------------- | --------------------------------------------------- |
| 1Password     | `op://DOMAIN/VAULT/ITEM/FIELD`                       | Optional section: `op://DOMAIN/VAULT/ITEM/SECTION/FIELD`. Uses the 1Password CLI. |
| Infisical     | `infisical://DOMAIN/PROJECT_ID/ENV_NAME/SECRET_NAME` | Nested secret names are allowed in the last path segment. The `/api` suffix is added automatically. |
| Doppler       | `doppler://DOMAIN/PROJECT_NAME/ENV_NAME/SECRET_NAME` | Uses the Doppler CLI.                               |
| Vault         | `vault://DOMAIN/MOUNT/KEY/FIELD`                     | Path segments are URL-decoded. Uses the Vault CLI.  |
| Passbolt      | `passbolt://DOMAIN/RESOURCE_ID/FIELD`                | Uses the Passbolt CLI.                              |
| File          | `file://PATH?query=.fields[0].field`                 | `?query=` is required. INI, JSON, YAML, and TOML.   |
| Shell command | `sh://gcloud auth print-access-token`                | Also `bash://`, `zsh://`, and `fish://`. Wrap: `fish --no-config`, `zsh -f`, `$BASH_ENV` cleared. |
| Inline value  | `"visible-in-lade-yml"`                              | Use `!` to force a raw value and `!!` to keep a leading `!`. |

Use `lade eval <uri>` to resolve one URI when debugging a provider.

`file://` details:

- `?query=` is a JSON path after the file is parsed (`access_json`). Examples:
  `.token`, `.db.password`, `.fields[0].field`, `.section.password` for INI.
- Path is relative to the `lade.yml` directory, or absolute. `~/` and `$HOME/`
  expand to the user home. Spaces in the path must be percent-encoded (`%20`).
- Extension selects the parser: `.json`, `.yaml` / `.yml`, `.toml`, `.ini`.
- A `file://` URI without `?query=` is rejected.

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
does not read `.bashrc` or login profiles anyway. Preexec (`lade set`) still
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

`sh://`, `bash://`, `zsh://`, and `fish://` sources can derive a value with the
shell. Lade recognizes simple `$NAME` and `${NAME}` references to build the
dependency graph, then passes their resolved values as environment variables to
the shell without rewriting the script. The shell remains responsible for all
other expansion syntax.

```yaml
"curl .*api\\.example\\.com.*":
  user: demo-user
  .password: op://company/api/password
  Authorization: 'sh://printf "Basic %s" "$(printf "%s:%s" "${user}" "$password" | base64 | tr -d "\n")"'
```

Quote shell variable expansions (`"$user"`, `"$password"`) so their values are
passed as single arguments. The shell provider output is treated as secret and
is masked like other provider-resolved values.

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

`when` is `always` (default), `human`, or `agent`. Audience comes from
`detect()`: `--pretool` or `lade hook` is `agent`; `lade set`/`unset` is
`human`. Leftover `LADE_VIA` is ignored. Via lives on the T ticket, not
the child env. Otherwise env signals (`AI_AGENT`, `CURSOR_AGENT`,
`CLAUDECODE`, not `CURSOR_VERSION`) select `agent`, else `human`. The
same pattern can be a YAML list of these blocks when `when` differs.
`silence` is optional and skips that rule's secret progress lines at
hydration.

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

Network providers acquire temporary local forwards for the command lifecycle.
Assign a URI to an environment variable for a dynamic local port, or to a number
for a fixed local port.

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
