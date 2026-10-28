# Secret cache

In-memory hub for hydrated secrets. `terraform plan` then
`terraform apply` can reuse one rule body without a second vault
round trip. Only the same Lade binary reads the hub, and no other
process of the user can read its memory.
Tickets stay in
[protocol.md](protocol.md). The diary stays in
[observability.md](observability.md). Env overrides are in
[env.md](env.md). The tunnels the hub runs are in
[tunnels.md](tunnels.md).

## Opt in

`ttl:` sits on that rule body's `.` block. Not on the URI. Not
folded like `log`. A home `ttl` does not opt in a child's `sh://`.

```yaml
"terraform .*":
  .:
    ttl: 5m
  AWS_ACCESS_KEY_ID: op://vault/item/field
```

Two command lines. Two `lade set` processes. They share if they
hit the same rule body. Two patterns, or two `when:` bodies under
the same pattern: the second refetches. It does not fail.

| Binding on that body | Default | Opt-in |
|---|---|---|
| Vault / `op` / cloud / file / sops / age | 5m | `ttl: off` disables. `ttl: 1h` … `ttl: 24h` (max) |
| `sh` / `bash` / `zsh` / `fish` | off | that body writes `ttl:` (same max) |
| This cwd (hub window) | yaml / 5m | `lade ttl set 2h` marks this cwd. Ancestors and children share it. `ttl: off` on the body still wins. Shell URIs opt in. `lade ttl unset` clears this cwd. |
| Tunnel | off | `ttl:` on the body or a `lade ttl set` window. Counts from the last command that used it |
| `raw://`, package | off | never |

A body `ttl:` longer than 24h fails at load. `lade ttl forget`
drops the RAM now. The next match refetches.

## Max cache

`max_cache` in `config.json` is the user's ceiling over every row
above: `"0"` (or `0`, `"off"`) keeps nothing past the command,
`"1h"` keeps at most 1h. No field: no ceiling. The hub enforces it,
not the client: a `Put` past it is clamped, a `Put` under `"0"` is
dropped, a window restamps at most to it, and a tunnel keeps at most
that once idle.

A watcher thread in the hub `stat`s `config.json` every second and
reads it only when its mtime, size, or inode moved, or when the last
read failed. Requests never touch the file: they read the value the
watcher last saw. When the value changes, in any direction, the next
request or nap drops every secret row and stops every tunnel no
command holds. Tunnels in use stay, capped. An unreadable file or an
invalid value counts as `"0"`. Durations go up to 24h, like `ttl:`.
`lade status` shows the value, or the error and `ok: false`.

One matching command can hit several patterns. One `Get` / `Put`
per cacheable body, not one for the whole command. `rule` on the
wire is that body's pattern string.

A withheld or `pending` disclaimer never `Put`s. Consent first.
A later hit still runs `resolve_disclaimers`.

## Who talks to it

`set`, inject, pretool wrap, and `mcp` connect and may spawn the
hub. `unset` may `UnlinkT` only.

A plain `set` (no secret, tunnel, disclaimer, or pinned tool)
never connects. For a human at a terminal, the first `PutT` of any
other `set` carries `notice`, and that `PutT` may spawn the hub, so
a repo that only pins packages still gets checks. `notice` starts
the GitHub release check when the last attempt is a day old (10s
timeout, on the hub's blocking pool), and hands out the hourly
slot. The slot owner refreshes hook files, then shows at most one
box: a new release, and hooks that need the user. The ledger (last
attempt, latest tag, last error) is RAM only, like the rest. A new
hub checks again.

A `set` that starts a harness (`opencode`, `claude`, `codex`,
`cursor-agent`, `agent`) refreshes and reports that harness's hooks
every time, without the hub.

`lade status` Pings a live hub and reads its ledger. `setup`,
`update`, and `upgrade` read the ledger for the release line.
`lade log prune --hub` sends `Quit`. None of them spawns.

`age-plugin-lade`, `lade eval`, `on`, `off`, and `bench` never
connect. The pretool handler (`lade hook` on stdin) writes the T
file only.

`CI` is not a signal.

## Lookup

The row key is `(cwd, yaml path, rule pattern, when, walk hash,
saved_user)`.

`yaml path` is the file that body came from. `when` is `always`,
`human`, or `agent`. Same pattern in `$HOME` and in the project:
two rows. Same pattern, two `when:` bodies: two rows.

`saved_user` is the resolved Lade user, or empty. Alice and Bob
do not share a row.

The `lade ttl set` window of this cwd is read once per command, and
only when a body could be cached under some window: a raw-only body
never calls the hub.

`walk hash` is SHA-256 of every `lade.yaml` / `lade.yml` from
CWD to `$HOME`. For each existing file, in walk order: 8-byte
big-endian path length, path UTF-8, 8-byte big-endian content
length, raw bytes. Comments change the hash. No mtime. Any edit
is a miss and a refetch.

## What the hub holds

```mermaid
flowchart LR
    Lade[lade set or wrap] -->|plaintext, same cdhash only| Hub[hub process]
    Hub -->|plaintext| Lade
    Lade -->|plaintext| Child[child env]
```

Two RAM tables. Every request first drops rows whose insert
time + `ttl_ms` is due. The clock keeps counting while the
machine sleeps (`CLOCK_MONOTONIC` on macOS, `CLOCK_BOOTTIME` on
Linux), so a closed lid does not stretch a TTL. `Put` on an
existing secret key upserts and refreshes that clock.

| Table | Cap | Evict |
|---|---|---|
| Secrets | 256 rows | TTL, then oldest insert if still full |
| Tickets | 128 rows | `UnlinkT`, or older than 1 h |

The cap is a fence for a client that puts faster than TTL. The
hub does not walk yaml, run providers, acquire tunnels, or write
the diary.

Values cross the socket and sit in the hub's RAM in plaintext, only
to and from a client the hub knows as its own binary (below). A row's
values are zeroed when it goes. A cache hit costs one socket round
trip: under 1 ms. A crash, logout, or upgrade is always cold.

## Memory guard

The hub caches a secret only where no other process of the user can
read its memory. Elsewhere it caches none: every command hydrates
from its providers, and the hub still holds tickets and tunnels. No
step needs `sudo` or an Apple account.

| Where | Cache | Guard |
|---|---|---|
| Linux | on | the hub sets itself not dumpable (`PR_SET_DUMPABLE` 0) at start: no `ptrace`, no `/proc/<pid>/mem` |
| macOS, signed with hardened runtime (release binaries, `scripts/install-local.sh`) | on | no debugger attaches |
| macOS debug build (tests, `cargo run`) | on | none: cargo does not sign |
| macOS, any other build (a plain `cargo install`) | off | |

Linux needs no signature: any process may make itself not dumpable.
macOS needs the hardened runtime flag in the binary's signature. Ad
hoc signing needs no Apple account. A `cargo install` turns the cache
on once signed:

```
codesign --force --sign - --options runtime "$(command -v lade)"
```

The hub also sets no core file. Root reads any process, on either OS.

## Who may talk to the hub

The hub serves a connection only when the kernel vouches for the
client:

| OS | Check |
|---|---|
| macOS, signed binary | The client's audit token, from `LOCAL_PEERTOKEN`, names that exact process: a reused pid or an `exec` since the connect fails. Its code must be valid and its cdhash equal the hub's |
| macOS, unsigned binary (an Intel `cargo build`) | Same uid, and the client's executable is the hub's file (device and inode) |
| Linux | Same uid (`SO_PEERCRED`), and `/proc/<pid>/exe` is the hub's file |

## Process

Hidden verb: `lade __hub`. Same binary, `setsid`, no stdin. It
stays on `accept`. Crash, logout, kill, inode mismatch, or
crate version mismatch drop it. Crash is always cold. Nothing
is serialized.

After 1h with no request, it prunes expired rows. It exits when
no secret, ticket, tunnel, or `lade ttl set` window is left. A window
keeps it up until `lade ttl unset`. Every minute it also checks
that clients can still reach it, and exits, rows or not, when its
socket path is gone or holds another hub's socket, or when a
reinstall put another file at its binary path (new clients hash
another token). On exit it unlinks the socket and its lock only if
the socket path is still its own.

A client that may use the hub:

1. Connects to `{cache_root()}/hub-{token}.sock` (`LADE_CACHE_DIR`
   or `ProjectDirs::from("com", "zifeo", "lade").cache_dir()`,
   mode `0600`). `token` is 16 hex chars of SHA-256 over this
   process's mapped image (`dev`, `ino`, size, mtime, first
   4 KiB). Same crate version, different file: different
   socket. `cargo run` and an install can both be up.
2. No full reply in 20 ms: miss, hydrate, at most one later
   `Put`. Ignore a late `Hit`.
3. Socket absent: `flock`
   `$CACHE/hub-{token}.lock`, spawn `lade __hub`, exclusive
   `bind`. The bind loser connects once to the winner. Spawn
   fail: hydrate as usual.

On `bind`, the hub snapshots its mapped image inode and holds
an fd on that file. On `accept` it drops a peer that fails the
check in [Who may talk to the hub](#who-may-talk-to-the-hub). Every
frame also carries `CARGO_PKG_VERSION`. Mismatch, or a
successful write with no decodable `Rep`: the client unlinks
its own socket and spawns once on a fresh 20 ms clock. It does
not touch another image's hub.

`lade upgrade` is a new token (new file) plus the version
frame. The old hub sees its binary replaced and exits within a
minute. `lade status` and `lade log prune
--hub` talk to this image only.

## Tickets

T has no secret values. The JSON file is still the peel
(`{cache}/tickets/{id}.json`, or `LADE_TICKET_DIR`). The
pretool handler writes the file only. Wrap / `unset` may
`PutT` / `UnlinkT` after. Hub T is optional.
`ticket_ready` and space-form `--pretool id` stay file-only.

## Wire

One connection is one request, one response, then close. The hub
serves one connection at a time and drops one that has not
finished after 500 ms. Crate:
`rkyv`. Magic `LDH1`, then `u32le` body length, then a
`WireReq` / `WireRep` (`ver` plus `Req` / `Rep`). `N == 0`,
`N > 1048576`, or bad magic: drop, no `Rep`. After a write,
that silent drop is a stale hub (unlink + spawn once).

| `Req` | `Rep` |
|---|---|
| `Get` | `Hit` or `Miss` |
| `Put` | `Ok` |
| `GetT` | `Ticket` or `Miss` |
| `PutT` / `UnlinkT` | `Ok` |
| `Ping` | `Stat` (`pid`, `secrets`, `tickets`) |
| `Quit` | `Ok`, then the hub exits and unlinks the socket |

`user` is `saved_user` or `""`. `path` is that body's yaml
file. `ttl_ms` is that body's duration, or 300000 when the
body used the vault default, or this cwd's hub window when
`lade ttl set` marked one. `ttl: off` bodies are not put.
`Err` 4 is denied (wrong peer or version).

## CLI

`lade status` has a Hub section: `pid …` or `down` / `stale`, the
`max_cache` in force, whether the cache is on and what guards it (or
why it is off, with the `codesign` command on macOS), key, tunnel,
and ticket counts, this cwd's window, key names, and tunnels.
`--json` adds extra `hub` (`state`, `pid`, `secrets`, `tickets`,
`window`, `keys`, `key_count`, `tunnels`, `max_cache_ms`, `memory`
with `state` `hardened_runtime`, `not_dumpable`, `debug_build`, or
`off` and its `detail`). The hub never sets `ok`. An invalid
`max_cache` does.

`lade ttl` lists this cwd's key names, the cwd the row is
valid for, yaml path, and ttl left. A window past `max_cache`
reads `8h, capped to 1h by max_cache`. `--global` lists every row
on this hub. Never values. `lade ttl set 2h` marks this cwd on
the hub. A parent or child cwd shares that window (nearest
path wins). `lade ttl unset` clears this cwd.
`lade ttl forget` and `lade ttl forget KEY` drop
rows. List and forget do not spawn. Set and unset spawn.
`lade log prune --hub` is an alias of `lade ttl forget`.
Combinable with `--keep`. Other `hub-*.sock` leftovers stay.
The next matching command refetches. A vault edit does not
evict a live row.

Progress tags a hub hit `(c)`. Overridden is `(o)`. Unset is
`(u)`. The diary sets `cached: true` on those public bindings.
Extra, like `agent`. See [cli.md](cli.md).

## What is not cached

- Shell URIs unless that body has `ttl:` or this cwd has a hub window
- Raw literals, tunnels, packages
- `lade eval` / `age-plugin-lade`
- A pending disclaimer treated as already consented
- Values in the diary, the ticket, or a hub log

## Failures

| Event | What happens |
|---|---|
| Hub dies mid-`Put` | This command uses its hydrate. Next misses |
| Hub dies, leftover T files | Files still work |
| Yaml edit between plan and apply | Miss, refetch |
| Provider value changed, hub still live | Same row until TTL or `lade ttl forget` |
| `max_cache` edited, up or down | Every secret row dropped, idle tunnels stopped |
| `max_cache` invalid or `config.json` unreadable | Cache off until fixed |
| Non-`lade` peer, or another build of it | Drop, `Err` 4 |
| No memory guard (a macOS build without hardened runtime) | No secret cached, tickets and tunnels still served |
| Inode or crate version mismatch | Unlink this image's socket, spawn once |
| Other image (`cargo run` vs install) | Other socket. Both hubs may stay up |
| Socket > 20 ms | Miss, hydrate, one `Put` |
| Table at cap, all TTLs live | Evict oldest insert |
| Machine sleeps past a TTL | Row is due at wake |
| Client connects and sends nothing | Dropped after 500 ms, next client served |
| Idle 1h, no row, no window | Exit, unlink own socket and lock |
| Idle 1h, rows or a window left | Stay on `accept` |
| Socket path gone or replaced (checked every minute) | Exit, leave the path alone |
| Binary replaced by a reinstall (checked every minute) | Exit, unlink own socket and lock |
