# Observability

Opt-in record of which access ran. Secret values are never stored.
Vault addresses (`op://…`, `kubectl://…`) are. `matches` is the public
keys and URIs, not values. `lade usage` lists rules that fired. Unused
rules are omitted.

See also [architecture.md](architecture.md) and [protocol.md](protocol.md).

## Turn it on

`log:` is last-wins. Absent `log` is not a vote. `log: false` punches
a hole on the rules it sits on.

```yaml
.:
  .:
    log: true
"^git status":
  .:
    log: false
```

A child `^git status` with `log: false` silences only `git status`.
It does not turn off `.: log: true` for `echo hi`.

| Situation | Flag used |
|---|---|
| At least one rule matched | last explicit `log` on **those** rules |
| No match | last explicit `log` on the **loaded walk** (`seen`) |

`LADE_EVENTS=off` skips writes. That is not `LADE_LOG` (`env_logger`).
A write failure never changes the command's exit.

## What gets a row

| Surface | Row |
|---|---|
| Shell wrap, inject, set, approve | `access` after hydrate, `denied` if a disclaimer is withheld, `seen` if nothing matched |
| Pretool handler, no match | `seen` if the walk says `log` |
| Pretool handler, match with nothing to inject | `seen` if those rules say `log`. No ticket. |
| Pretool handler, match that wraps | no (the wrap writes) |
| MCP `tools/call` (`lade hook`) | `seen` |
| `lade mcp` (server start) | same as inject |
| `lade eval`, `age-plugin-lade` | `access`. Always on. No `lade.yaml` `log` flag. |
| unset, status, bench, log, usage, on, off | no |

Starting an MCP server is not a hook. A row exists only if the
process is `lade mcp -- …` (or a URL). Each later `tools/call` is a
separate verb row, whether or not that prefix was used.

`via` is `preexec`, `pretool`, `mcp`, `organic`, or `unknown`.
Audience is `human` or `agent`. Hydrate stays on the child. The diary
only keeps the scrubbed `command`, `argv`, and public `matches`.

## Shape

`command` is argv0. `argv` is JSONB: a string array for a shell or
`lade mcp` line, an object for an MCP verb. Each string is capped at
1024 Unicode scalars. `command_truncated` is only the argv0 cap.

```json
{
  "kind": "access",
  "via": "pretool",
  "command": "npm",
  "argv": ["run", "deploy"],
  "matches": [
    {
      "file": "/proj",
      "rule": "^npm run deploy",
      "bindings": [
        {
          "key": "API_TOKEN",
          "uri": "op://prod/api/credential",
          "family": "secret",
          "bin": "op",
          "version": "2.31.0"
        }
      ]
    }
  ]
}
```

```json
{
  "kind": "seen",
  "via": "mcp",
  "command": "engram.mem_stats",
  "argv": { "project": "lade" }
}
```

```json
{
  "kind": "access",
  "via": "mcp",
  "command": "engram",
  "argv": ["--stdio"]
}
```

`matches` lists public keys and their `lade.yaml` URIs, not values.
`file` is the directory of that yaml. Each binding has `family`
(`secret` / `tunnel` / `bin`). Implied or explicit bins add `bin`
and, when the lock beside that yaml agrees, `version`. Winners only.
`.NAME` is omitted. `agent` is optional hook metadata (`harness`,
`tool`, `session`, …). Keys may be absent.

Scrub: leading `NAME=value` is stripped, hydrated values become
`${NAME}`, leftover hydrate stores `""`, then needles / prefixes /
long tokens become `?`. Child stdout and `lade eval` are never
stored.

## Read

Default window is 90 days. `--since` / `--until` are durations back
from now (`Ns | Nm | Nh | Nd | Nw | Nmonth`; `m` is minutes).
`--limit` is an extra cap. Bare `--limit 20` drops the 90-day
default. Queries stay on the current git root (worktrees count).
`--all` / `--global` read every repo. `--path` scopes to another tree.

| Command | Meaning |
|---|---|
| `lade log` | Newest first. Prints `command` plus `argv`. Warns if the chain is broken |
| `lade log --group command` | Frequency by argv0 |
| `lade log --kind` / `--audience` | Filters |
| `lade usage` | Matched `lade.yaml` rules in this tree, most frequent first. Warns if the chain is broken |
| `lade log prune --keep` | The only delete. Starts a new chain epoch over what remains |
| `lade log share` | Gzipped snapshot (`lade-$USER-$FROM-$TO.tar.gz`) with its own chain |
| `lade log verify` | Walk the hash chain. `--source` checks a pack |
| `--source` | Read packs (or `local`) without writing the live db |
| `--json` | Stored fields |

`lade usage` omits unused rules and catch-all `.`. The walk stops at
`$HOME`. `lade log` and `lade usage` do not write rows. A broken
chain prints a warning that names the tampered row and how many
rows still verify before and after it. The listing still prints.
`lade log verify` exits non-zero.

## Store

One SQLite WAL:

`ProjectDirs::from("com", "zifeo", "lade")` `data_local_dir()/events.db`

`lade log --help` and `lade status` print the path. Tests set
`LADE_EVENTS_PATH`. `status --json` adds `log` (`path`, `events`,
`bytes`) and leaves `ok` unchanged.

`matches`, `agent`, and `argv` are JSONB (`jsonb(...)` on write,
`json(...)` on read). SQLite has no JSONB storage class, so
`PRAGMA table_info` may show `BLOB`. The values are still JSONB.

Each row is HMAC-SHA256 chained to the previous one. The key is
compiled into this Lade version. A raw `sqlite3` edit of a past
row breaks `lade log verify`. Parallel writers take an Immediate
transaction, so they append one at a time. `lade log share`
reseals the redacted snapshot. That pack verifies on its own.
The chain is tamper-evident, not tamper-proof: anyone who runs
this binary can reseal.

The diary row is not T. T is the 4-character tmp file used to run
providers. See [protocol.md](protocol.md).
