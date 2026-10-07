# CLI

User-facing verbs a person or a harness types. Internal protocol
(`set`, `unset`, `hub`, stdin `hook`, `--pretool`) stays out of
`lade --help`. `lade --help -v` lists them. Details:
[protocol.md](protocol.md).

Spoken words: **harness**, **pre-exec**, **pre-tool**. Diary JSON
may still say `agent`. `lade status --json` keeps `version`,
`global_config`, `hooks` (`preexec` plus `pretool`),
`project_config`, and `ok`.

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
Lade connected: 1Password vault: AWS_ACCESS_KEY_ID (c) 0 ms
```

`silence: true` on a rule hides that rule's progress lines.

## Three stores

| Store | See | Forget |
|---|---|---|
| RAM keys (this binary) | `lade cache` | `lade cache forget` |
| Diary | `lade log` | `lade log prune --keep 90d` |
| On-disk isolation (mise, tickets) | `lade teardown --global` | same |

A vault edit does not evict RAM. `status` prints a count.
`lade cache` prints the names.

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
until `lade hook disable --scope user` (`lade --help -v`).

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
mise, providers, diary size, hub pid and counts.

```bash
lade status
lade status --json
lade status --all
```

```
hub: pid 18432 (2 secrets, 0 tickets)
```

`--json` extra `hub`: `state`, `pid`, `secrets`, `tickets`.
`ok` is unchanged.

### `lade cache`

This binary's in-memory secret cache. Names only. Never values.
Does not spawn.

```bash
lade cache
lade cache list
lade cache list --json
lade cache forget
lade cache forget AWS_ACCESS_KEY_ID
```

```
hub: pid 18432

AWS_ACCESS_KEY_ID  terraform .*  ~/code/claryo/lade.yml  3m left
DATABASE_URL       terraform .*  ~/code/claryo/lade.yml  3m left
```

Hub down / off / stale:

```
hub: down
```

`lade log prune --hub` is an alias of `lade cache forget` (no
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

- [cache.md](cache.md) hub, TTL, wrap key
- [observability.md](observability.md) diary
- [env.md](env.md) overrides
- [architecture.md](architecture.md) wrap flow
- [protocol.md](protocol.md) tickets
