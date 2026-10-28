# CLI

User-facing verbs a person or a harness types. `lade --help` is
the playbook (examples, RAM ttl, loader marks). Each verb:
`lade <command> --help`. Internal protocol (`set`, `unset`,
`hub`, stdin `hook`, `--pretool`) stays out of `lade --help`.
`lade --help -v` lists them. Details: [protocol.md](protocol.md).

Spoken words: **harness**, **pre-exec**, **pre-tool**. Diary JSON
may still say `agent`. `lade status --json` keeps `version`,
`global_config`, `hooks` (`preexec` plus `pretool`; `preexec` is
`null` when no shell started Lade), `project_config`, and `ok`.

## Loader marks

While secrets hydrate, a progress line can tag a binding:

| Mark | Meaning |
|---|---|
| `(c)` | Cached. Value came from this binary's in-memory hub, not the vault. |
| `(o)` | Overridden. A closer `lade.yaml` replaced a parent binding. |
| `(u)` | Unset. This command's rule cancelled the binding (`~`). |
| `(o, c)` | Both overridden and cached. |

Example:

```
✔︎ Vault 127.0.0.1:8200: TOKEN (c)                                        0 ms
```

`silent: true` on a rule hides that rule's progress line in a human's
terminal (pre-exec `set`, a direct `inject`). It is for humans: an
agent never gets progress lines.

## What an agent reads

An agent (a pre-tool wrap, or `AI_AGENT`, `CLAUDECODE`, … in the env)
reads stderr as context, so Lade keeps it to what it needs to act:

| Case | stderr |
|---|---|
| Secrets loaded | nothing. Output shows `${API_TOKEN:-REDACTED}` where a value was |
| A provider or tunnel failed | `lade: error: Could not load secrets from one provider: <cause> The command did not run. …` |
| A warning | `lade: warning: <text>` |
| A rule disclaimer | `lade: warning: Disclaimer required. … Only a human can approve it: ask them to run this command in their terminal.` No approve code |
| A new Lade, or hooks to fix (hourly, when the wrap uses the hub) | `lade: Tell the human: <notice>` |

Every message is one line starting with `lade:`, with no box, colour,
or keyboard advice.

## Three stores

| Store | See | Forget |
|---|---|---|
| RAM keys and tunnels (this binary) | `lade ttl` | `lade ttl forget` |
| Diary | `lade log` | `lade log prune --keep 90d` |
| On-disk isolation (mise, tickets) | `lade teardown --global` | same |

A vault edit does not evict RAM. `status` prints a count.
`lade ttl` prints the names.

## Public commands

### `lade setup`

Wire this repo: locked packages, first-time pre-exec, repo pre-tool.

```bash
lade setup
lade setup --harness cursor
lade setup --unlock
```

### `lade update` / `lade upgrade`

`update` re-resolves ranged pins and rewrites the lock.
`upgrade` installs a newer Lade binary.

```bash
lade update
lade upgrade
lade upgrade --version 0.19.2 -y
```

### `lade teardown`

Without a flag: remove this repo's pre-tool hooks and run
`?teardown=` commands. The shell hook stays.

`--global`: wipe this machine's on-disk cache (mise isolation,
tickets, sidecars). Not the RAM hub. Home pre-tool hooks stay
until `lade hook disable --global` (`lade --help -v`).

```bash
lade teardown
lade teardown --global
```

### `lade on` / `lade off`

Print snippets for this shell. `eval "$(lade on)"`.

### `lade add` / `lade remove`

Edit the nearest `lade.yaml`. `add` then runs setup.

```bash
lade add secret --rule 'terraform .*' --key AWS_ACCESS_KEY_ID --uri op://v/i/f
lade remove secret --key AWS_ACCESS_KEY_ID
```

### `lade user`

Per-user yaml map key. `--reset` drops it (OS user).

```bash
lade user alice
lade user --reset
```

### `lade status`

Inventory: version, age-plugin, home config, pre-exec, pre-tool,
mise, providers and their timeouts, diary size, and the hub: this
cwd's window, cached key names, and tunnels. Never values. Does
not spawn a hub.

```bash
lade status
lade status --json
lade status --all
```

```
Hub
  state      pid 18432
  max cache  1h
  cache      2 keys (1 for this cwd), 1 tunnels, 0 tickets
  window     2h for this cwd, capped to 1h by max_cache
  KEY                CWD        TTL
  AWS_ACCESS_KEY_ID  ~/src/app  3m left
  PORT   KEY       URI                                        STATE
  18080  API_PORT  ssh://jump.example.com/api.internal/8080  42m left
```

`--json` extra `hub`: `state`, `pid`, `secrets`, `tickets`,
`window`, `keys` (`name`, `cwd`, `ttl_left_ms`), `key_count` (every
cwd), `tunnels` (`key`, `uri`, `port`, `leases`, `ttl_left_ms`,
`phase`), and `max_cache_ms`, the cap the hub enforces now. Each
provider has `timeout_s`. `global_config` adds `max_cache` (raw),
`max_cache_ms`, and `max_cache_error`. An invalid `max_cache` sets
`ok` to false: the hub then keeps nothing.

### `lade ttl`

This binary's hub: cached secret names and tunnels. Names only.
Never values. Does not spawn.

```bash
lade ttl
lade ttl --global
lade ttl list
lade ttl list --json
lade ttl set 2h
lade ttl unset
lade ttl forget
lade ttl forget AWS_ACCESS_KEY_ID
lade ttl forget 15432
```

`lade ttl set` marks this cwd on the hub. A parent cwd
shares it. Secrets and tunnels of matching rules keep that long.
Vault and file stay in RAM 5m until then. Tunnels have no default.
`lade ttl unset` clears this cwd.
`lade ttl forget` drops the RAM and stops every tunnel now.
`lade ttl forget KEY` drops that secret name or tunnel key.

```
hub: pid 18432

AWS_ACCESS_KEY_ID  ~/src/app  ~/src/app/lade.yml  3m left

15432  15432  kubectl://k8s.example.com:6443/prod/default/service/postgres/5432  in use (1)
18080  API_PORT  ssh://jump.example.com/api.internal/8080  7h left
```

Tunnel rows: local port, key, URI, then `starting`, `failed`,
`in use (N)` leases, or the ttl left once idle. `--json` adds
`tunnels`, each with `phase` (`starting`, `ready`, `failed`).

Hub down / off / stale:

```
hub: down
```

`lade log prune --hub` is an alias of `lade ttl forget` (no
names). Mechanics: [cache.md](cache.md).

### `lade log` / `lade usage`

Diary. [observability.md](observability.md).

```bash
lade log
lade log --since 12h --json
lade log prune --keep 90d
lade log prune --hub
lade log share
lade log verify
lade usage
```

### `lade -- <cmd>` / `lade mcp` / `lade eval`

One-shot wrap, MCP wrap, or one URI to stdout.

```bash
lade -- tofu apply
lade mcp -- npx -y @modelcontextprotocol/server-everything
lade mcp https://example.com/mcp
lade eval op://vault/item/field
```

### `lade approve`

After a disclaimer. Exit `3` when withheld.

```bash
lade approve ab12c
```

### `lade bench`

Time parse, match, and each rule's hydrate. Hidden from
`lade --help`. Still: `lade bench --help`.

```bash
lade bench
lade bench --json --timeout 5s
```

## See also

- [cache.md](cache.md) hub, TTL, who may read the hub
- [tunnels.md](tunnels.md) hub tunnels, ports, leases
- [observability.md](observability.md) diary
- [env.md](env.md) overrides
- [architecture.md](architecture.md) wrap flow
- [protocol.md](protocol.md) tickets
