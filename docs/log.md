# Local command diary

Opt-in record of commands that went through Lade. Secret values are
never stored. Vault addresses (`op://…`, `kubectl://…`) are.

See also [architecture.md](architecture.md) and [protocol.md](protocol.md).

## When a row is written

`log:` is last-wins. Absent `log` is not a vote. `log: false` punches
a hole.

| Situation | Flag used |
|---|---|
| Command matched at least one rule | last explicit `log` on **those** rules |
| No match at all | last explicit `log` on the **loaded walk** (`seen`) |

A child `^git status` with `log: false` silences only `git status`.
It does not turn off `.: log: true` for `echo hi` or `npm run deploy`.

| Surface | Writes |
|---|---|
| Pretool handler, no match | `seen` if walk `log` |
| Pretool handler, match | no (wrap writes) |
| Pretool wrap, inject, set, approve | `access` / `denied` / `seen` if the flag above is true |
| Unset, eval, status, bench, log, usage, on, off | no |
| MCP | no |

`lade set` and inject write after hydrate, before the child (or
the naked shell command). An `unset` crash or a killed wrap still
has the row. A disclaimer withhold is `denied`. Hydrate is
`access`. Empty `matches` is `seen`.

Write failures never change the user command's exit. `LADE_EVENTS=off`
skips writes. That is not `LADE_LOG` (`env_logger`).

## Store

One global SQLite WAL:

`ProjectDirs::from("com", "zifeo", "lade")` `data_local_dir()/events.db`

Tests set `LADE_EVENTS_PATH`. `lade log --help` and `lade status`
print the path. `status --json` keeps `ok` unchanged and adds `log`
(`path`, `events`, raw `bytes` of db + wal + shm).

```
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA busy_timeout = 250;
```

`BEGIN IMMEDIATE; INSERT; COMMIT`. Busy longer than 250 ms drops the
row (`log::debug`). Schema migrates with `rusqlite_migration` on open.

```sql
CREATE TABLE events (
  id TEXT PRIMARY KEY,
  ts TEXT NOT NULL,
  kind TEXT NOT NULL,
  via TEXT,
  audience TEXT,
  actor TEXT,
  repo TEXT,
  git_commit TEXT,
  command TEXT NOT NULL,
  command_truncated INTEGER NOT NULL,
  hydrate_ms REAL,
  matches BLOB NOT NULL,
  agent BLOB
);
```

| Field | Meaning |
|---|---|
| `id` | uuid v7. Not the T ticket id |
| `ts` | RFC3339 UTC, ms |
| `kind` | `seen` / `access` / `denied` |
| `via` | `preexec` / `pretool` / `organic` / `unknown` |
| `audience` | `human` / `agent` |
| `actor` | Lade user, else `$USER` / `$USERNAME` |
| `repo` | Git root from `cwd`, or null. File reads under `.git/`, no `git` process |
| `git_commit` | `HEAD` hex, or null |
| `command` | Scrubbed text. Empty if a hydrated value is still a substring |
| `command_truncated` | Cut at 1024 Unicode scalars |
| `hydrate_ms` | Hydrate clock, `access` only |
| `matches` | JSON array, overlay order |
| `agent` | Free-form JSON (`harness`, `model`, `session`, later keys). Null if empty. Hook keys win over env. New keys do not need a schema change |

`matches` example:

```json
[
  {
    "file": "/proj/lade.yml",
    "rule": "^npm run deploy",
    "bindings": [
      {"key": "API_TOKEN", "uri": "op://prod/api/credential"}
    ]
  }
]
```

Public keys only. `.NAME` is omitted. The URI is the `lade.yml`
string, not a value.

## Scrub

Same pipeline on `seen`, `access`, and `denied`.

1. Strip leading `NAME=value` (`LADE_APPROVE=…`).
2. On `access`, replace hydrated public values with `${NAME}`.
3. If a hydrated value is still a substring, store `""` and stop.
4. Cap at 1024 scalars.
5. Context needles (`authorization:`, `bearer `, `x-api-key:`,
   `access_token=`, …): replace the next credential run with `?`.
6. Prefix keywords (`AKIA`, `sk-`, `ghp_`, `eyJ`, …).
7. Tokens ≥ 20 scalars with a letter and a digit become `?`.

Never store secret values, `.NAME`, `LADE_RESTORE`, child stdout, or
`lade eval` output.

## Read

Default window is 90 days. `--since` / `--until` are durations back
from now (`Ns | Nm | Nh | Nd | Nw | Nmonth`; `m` is minutes).
`--limit` is an extra cap. Bare `--limit 20` drops the 90-day
default. Queries stay on the current git root (worktrees count).
`--all` reads every repo. `--path` scopes to another tree.

| Command | Meaning |
|---|---|
| `lade log` | Typed commands, newest first |
| `lade log --group command` | Commands by frequency |
| `lade log --kind` / `--audience` | Filters |
| `lade usage` | Matched `lade.yml` rules in this tree, most frequent first, with the file path |
| `lade log prune --keep` | The only delete |
| `lade log share` | Gzipped SQLite snapshot (`lade-$USER-$FROM-$TO.tar.gz`) |
| `--source` | Read packs (or `local`) without writing the live db |
| `--json` | Stored fields |

`lade usage` is Lade usage, not a catalog. Unused rules and catch-all
`.` are omitted. There is no npm / make / `scripts/` walk. `lade.yml`
walk stops at `$HOME`. `lade log` and `lade usage` do not write rows.

## T

The diary row is not the ticket. T is a 4-character tmp file used to
run providers without a second YAML walk. The event `id` is a new
uuid v7 at INSERT. See [protocol.md](protocol.md).
