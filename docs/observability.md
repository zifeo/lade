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

The diary is not `LADE_LOG` (`env_logger`).
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

A wrap opens the diary on a thread while the command gets ready
(disclaimers, providers), then writes its row on that connection once
the child has started, or failed to start, and before it waits for
the child. The child's exit status never changes the row: a failing
command keeps it. Lade killed between the start of the child and the
commit (a few ms) loses the row. Ctrl-C does not: Lade forwards it to
the child and finishes the write. Pre-exec `set` writes its row
before the shell runs the command. A human who presses Enter to run a
command without Lade after a provider failure gets no row.

Starting an MCP server is not a hook. A row exists only if the
process is `lade mcp -- …` (or a URL). Each later `tools/call` is a
separate verb row, whether or not that prefix was used. When a
second hook of the same call cannot fill the first row, because
another row was appended in between, it appends a full copy: `lade
log` and `lade usage` count a `tool_use_id` once, its newest row.

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
          "package": "op",
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
(`secret` / `tunnel` / `package`). Implied or explicit packages add
`package`
and, when the lock beside that yaml agrees, `version`. Winners only.
`.NAME` is omitted. `agent` is optional hook metadata (`harness`,
`tool`, `session`, …). Keys may be absent.

Scrub: leading `NAME=value` words are stripped whole, a quoted value
with blanks included, hydrated values become
`${NAME}`, leftover hydrate stores `""`, then needles / prefixes /
long tokens become `?`. Child stdout and `lade eval` are never
stored.

## Read

Default window is 90 days. `--since` / `--until` are durations back
from now (`Ns | Nm | Nh | Nd | Nw | Nmonth`; `m` is minutes).
`--limit` is an extra cap. Bare `--limit 20` drops the 90-day
default. Queries stay on the current git root (worktrees count).
`--all` / `--global` read every repo. `--path` scopes to another tree.
Outside a git root, `lade log`, `lade usage`, `lade log share` and
`lade log prune --keep` exit 1 unless `--all` or `--global` is given.

| Command | Meaning |
|---|---|
| `lade log` | Newest first. Prints `command` plus `argv`. Warns if the chain is broken |
| `lade log --group command` | Frequency by argv0 |
| `lade log --kind` / `--audience` | Filters |
| `lade usage` | Matched `lade.yaml` rules in this tree, most frequent first. Warns if the chain is broken |
| `lade log prune --keep` | The only diary delete. Starts a new chain epoch over what remains. Warns if the chain is broken |
| `lade log prune --hub` | Alias of `lade ttl forget`. Next hydrate refetches. Does not spawn. Combinable with `--keep` |
| `lade log share` | Gzipped snapshot (`lade-$FROM-$TO.tar.gz`) with its own chain. Holds no user name and no absolute path of your machine ([Share](#share)). Warns if the chain is broken. Not in a harness session |
| `lade log verify` | Walk the hash chain. `--source` checks a pack |
| `--source` | Read packs (or `local`) read-only, without writing the live db |
| `--json` | Stored fields |

`lade usage` omits unused rules and the catch-all rules `.` and `.*`. A rule counts only
when it owns a winning public binding, so a rule that only sets `log`
or a disclaimer is not listed. The walk stops at
`$HOME`. `lade log` and `lade usage` do not write rows. A broken
chain prints a warning that names the tampered row and how many
rows still verify before and after it. The listing still prints.
`lade log verify` exits non-zero.

## Share

A pack is for someone else, so it names your files relative to the
repo or home they ran in. Each row drops `actor`, keeps `repo` as the
repo's directory name, and rewrites every absolute path in every other
field (`command`, `argv`, `agent`, each match's `file`, `rule` and
binding URIs, `file://` included): a path under the repo becomes
relative to it (`.` for the repo itself), one under your home becomes
`~/...`, and any other keeps its last component (`/opt/tools/tf`
becomes `tf`). A path is a `/` that opens a value or follows a space,
a quote, `=`, `:` or another separator, and runs to the next one; your
repo or home also counts inside a word, as in `-I/Users/me/inc`. A
URL's host part, as in `https://host/path`, stays. The default name
carries only the time range: `lade-$FROM-$TO.tar.gz`.

## Store

One SQLite WAL:

`ProjectDirs::from("com", "zifeo", "lade")` `data_local_dir()/events.db`

`lade log --help` and `lade status` print the path. Tests set
`LADE_EVENTS_PATH`. `status --json` adds `log` (`path`, `events`,
`bytes`) and `hub` (`state`, `pid`, `secrets`, `tickets`). Both
leave `ok` unchanged. `hub` Pings a live daemon. It does not spawn.

`matches`, `agent`, and `argv` are JSONB (`jsonb(...)` on write,
`json(...)` on read). SQLite has no JSONB storage class, so
`PRAGMA table_info` may show `BLOB`. The values are still JSONB.

Each row is HMAC-SHA256 chained to the previous one. The key is
compiled into Lade. Each seal also names the version that wrote it,
so a newer Lade still verifies older rows. A raw `sqlite3` edit of
a past row breaks `lade log verify`, and so does a row whose seal
was removed: Lade seals unsealed rows only once, when it adds the
chain to a diary written before 0.19.0. Parallel writers take an Immediate
transaction, so they append one at a time. `lade log share`
reseals the redacted snapshot. That pack verifies on its own.
The chain is tamper-evident, not tamper-proof: anyone who runs
this binary can reseal.

The diary row is not T. T is the 4-character tmp file used to run
providers. See [protocol.md](protocol.md).
