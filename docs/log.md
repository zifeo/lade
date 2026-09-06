# Local command diary

Opt-in record of commands that went through Lade. Secret values are
never stored. Vault addresses (`op://…`, `kubectl://…`) are.

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
| Pretool handler, match | no (the wrap writes) |
| MCP `tools/call` (`lade hook`) | `seen` |
| `lade mcp` (server start) | same as inject |
| unset, eval, status, bench, log, usage, on, off | no |

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
      "file": "/proj/lade.yml",
      "rule": "^npm run deploy",
      "bindings": [{ "key": "API_TOKEN", "uri": "op://prod/api/credential" }]
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

`matches` lists public keys and their `lade.yml` URIs, not values.
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
`--all` reads every repo. `--path` scopes to another tree.

| Command | Meaning |
|---|---|
| `lade log` | Newest first. Prints `command` plus `argv` |
| `lade log --group command` | Frequency by argv0 |
| `lade log --kind` / `--audience` | Filters |
| `lade usage` | Matched `lade.yml` rules in this tree, most frequent first |
| `lade log prune --keep` | The only delete |
| `lade log share` | Gzipped snapshot (`lade-$USER-$FROM-$TO.tar.gz`) |
| `--source` | Read packs (or `local`) without writing the live db |
| `--json` | Stored fields |

`lade usage` omits unused rules and catch-all `.`. The walk stops at
`$HOME`. `lade log` and `lade usage` do not write rows.

## Store

One SQLite WAL:

`ProjectDirs::from("com", "zifeo", "lade")` `data_local_dir()/events.db`

`lade log --help` and `lade status` print the path. Tests set
`LADE_EVENTS_PATH`. `status --json` adds `log` (`path`, `events`,
`bytes`) and leaves `ok` unchanged.

`matches`, `agent`, and `argv` are JSONB (`jsonb(...)` on write,
`json(...)` on read). SQLite has no JSONB storage class, so
`PRAGMA table_info` may show `BLOB`. The values are still JSONB.

The diary row is not T. T is the 4-character tmp file used to run
providers. See [protocol.md](protocol.md).
