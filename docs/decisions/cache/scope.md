# What the secret cache holds

A matched command fetches its secrets from their providers, and a provider can be slow or ask a person: 1Password wants Touch ID, age and SOPS a hardware key touch, a cloud CLI a network round trip. `terraform plan` then `terraform apply` would ask twice within a minute. Lade keeps what a provider resolved in the hub, a process of the same binary, in RAM, for a short ttl, so the next matching command reads it from there (`README.md`, Secret; `docs/cache.md`, top). The hub process itself, who may talk to it, its memory guard, its wire and its lifecycle are decided with the hub; these notes decide the cache policy: this one what is cached, under which key, what a user sees of it and what the hub's memory keeps, and [ttl](ttl.md) for how long and what the user can cap.

The problem is that a cache of secrets is a second copy of the secrets, and every rule of it is a place where access outlives its purpose or reaches the wrong command. A value kept too long outlives a rotation in the vault. A value keyed too loosely reaches another worktree, another teammate on the same machine, or a command whose `lade.yaml` was just edited to point elsewhere. A shell snippet may print something different on every run, so caching it by default changes what the command gets. A user who wants nothing kept must be able to say so once, for every repo, without trusting each `lade.yaml`. And a value the hub drops must not linger in its memory.

## What is cached

"Every value a provider resolves (vault, 1Password, file, sops, age, cloud) stays in RAM for 5 minutes" (`README.md`, Secret). A shell URI is different: its output is whatever the snippet prints, so it is cached only when the body asks with `ttl:` or the directory has a window (`README.md`, Secret; `docs/cache.md`, Opt in). A raw value is already in the yaml, and a package or a tunnel is not a value at all: "Raw and packages never cache" (`README.md`, Secret), and a tunnel is kept by its own ttl, decided with the tunnels. `src/config/ttl.rs` holds the eligibility, tested by `vault_defaults_to_five_minutes`, `explicit_ttl_opts_in_shell` and `session_window_opts_in_shell`.

`ttl:` sits on one body's `.` block, "Not folded like `log`. A home `ttl` does not opt in a child's `sh://`" (`docs/cache.md`, Opt in): a home file that caches its own vault reads must not decide that every repo's shell snippets are cached too.

A command none of whose bodies could cache anything, even under a window, does not call the hub for its secrets: "a raw-only body never calls the hub" (`docs/cache.md`, Lookup). That keeps a plain command at its cost with no socket round trip and no spawn.

**§ <a id="b0wwfx9g0">b0wwfx9g0</a> The hub caches the secrets a provider resolves, and those alone.**

- (a) Every secret provider URI is cacheable by default, except a shell URI (`sh://`, `bash://`, `zsh://`, `fish://`), which is cacheable only when its own body sets a `ttl:` duration or a `lade ttl set` window covers the directory.

- (b) **Retired.**

- (c) A body's `ttl:` opts in that body alone, whatever the `ttl:` of another body, a parent file's included, says.

- (d) A command asks the hub for its secrets when one of its bodies could cache a value under some window, and otherwise fetches them from their providers straight away.

## Which command a value reaches

The row key is the directory, the yaml file of the body, the rule pattern, its `when`, the walk hash and the Lade user (`docs/cache.md`, Lookup). Each part keeps a value from reaching a command it was not fetched for. "Rows are keyed by working directory, so each worktree has its own" (CHANGELOG 0.20.0, Secret cache). "Alice and Bob do not share a row" (`docs/cache.md`, Lookup): a per-user map gives each teammate their own value, [dcrxp986s](../config/bindings.md#dcrxp986s). The same pattern in the home file and in the project, or two `when:` bodies, are two rows, and the second fetches again rather than fail (`docs/cache.md`, Opt in).

The CHANGELOG lets one pattern hold "a list of bodies with different `when`" (0.18.0), and the load also takes two bodies with the same `when`: a `log`-only body beside a caching one under `.` is such a list, and it works (`two_dot_bodies_keep_the_shell_secret`, `log_then_shell_ttl_body_roundtrips_hub`). Two such bodies that both cache a name share one row, since the key holds the `when` and not the body: each `Put` replaces the other's names, so they miss and fetch more often. A hit still serves a body only its own names, (c), so neither gets the other's values.

The walk hash is a SHA-256 over every `lade.yaml` from the directory up to `$HOME`, path and bytes, "Comments change the hash. No mtime. Any edit is a miss and a refetch" (`docs/cache.md`, Lookup; `comment_changes_the_hash`). A yaml edit that points a key to another vault item must not be answered with the old item's value, and comparing bytes is the one check that cannot be fooled by a preserved mtime. The hash covers every file on the walk, so it also covers a binding that another body builds from a cached one with `${NAME}`: an edit to either definition misses every row of the walk. It does not cover the value of the referenced binding: a dependent cached in its own row keeps the value it was built from until its own ttl, like any row ([hadg0brg4](ttl.md#hadg0brg4)).

A row holds what the body bound when it was stored. A later body on the walk may override, cancel or retype one of its names, and a window that went away may make a shell name uncacheable again; the hit must then give that name up, or the command would get a value its yaml no longer says.

**§ <a id="je3g2tk0d">je3g2tk0d</a> A cached value is served only to a command run in the same directory, from the same `lade.yaml` files, rule body and Lade user as the command that fetched it.**

- (a) Each body keeps its own row: the same pattern in two `lade.yaml` files, or two bodies of one rule with different `when`, each fetch and keep their own.

- (b) The walk hash covers the path and every byte of every `lade.yaml` on the walk, so any edit to one of them, a comment included, misses.

- (c) A hit serves only the names the body caches for this command: a name a later body overrides, cancels or turns into a tunnel or package, or a name no longer cacheable, is fetched as usual.

## What a user sees

The cache must be inspectable without becoming a way to read secrets: "`lade ttl` | This binary's hub: cached key names and tunnels. ... Never values" (`README.md`, Commands; `docs/cli.md`, `lade ttl`: "Names only. Never values"). Anything that can talk to the hub is the same binary, [keyfb5k7w](../promises.md#keyfb5k7w), but what `lade ttl` prints lands in a terminal, a CI log or an agent's context. A hit still hands its values to the command that asked; what the listing keeps out is the value itself. CHANGELOG 0.20.0 says `lade ttl` lists "this cwd's key names"; it lists the rows of the directories a `lade ttl set` window of this one shares, "Ancestors and children share it" (`docs/cache.md`), as `docs/cli.md` now says (`default_list_keeps_this_cwd_its_ancestors_and_children`). A tunnel belongs to the hub, not to a directory, so every listing shows every tunnel.

**§ <a id="f1t0s88fs">f1t0s88fs</a> Lade shows which names the hub holds and how long they have left, in a listing that carries no value.**

- (a) A row of the hub's listing carries a name, its key and its time left, and `lade ttl` prints from those rows.

- (b) `lade ttl` lists the secret rows of this directory, its ancestors and its descendants, and `lade ttl --global` every secret row.

A value served from the hub skipped the provider, so the provider's own audit log does not show that use; Lade's must. The progress line tags it `(c)` (`README.md`, Commands; `secret_progress_groups_mark_cached_keys`), and "a binding served from the hub has `cached: true` in its diary row" (CHANGELOG 0.20.0, Cached diary rows), for `lade eval --ttl` too (CHANGELOG 0.20.1; `eval_ttl_keeps_the_value_in_the_hub`). When the diary records at all is [gb0y44ydb](../promises.md#gb0y44ydb).

**§ <a id="cj4kjwcp7">cj4kjwcp7</a> A binding served from the hub is marked as cached wherever Lade reports it.**

- (a) A human's progress line tags it `(c)`.

- (b) Its diary row carries `cached: true`, on a wrap, in `lade mcp` and on `lade eval --ttl` alike.

## What the hub's memory keeps

The values sit in the hub's RAM in plaintext (`docs/cache.md`, What the hub holds), so freeing a row must not leave them in memory the allocator hands out again: "A row's values are zeroed when it goes" (`docs/cache.md`; `src/hub/tables.rs`, "A row leaves no value behind in freed memory"). A row goes when its ttl ends, when a `Put` replaces it, when the table is full, when `max_cache` changes, and when it is forgotten, by name or whole. Copies made to answer a request, on the wire and in the client, are outside this promise: the client hands the value to the command anyway.

**§ <a id="d3r0zsk5r">d3r0zsk5r</a> The hub overwrites a cached value with zeros before it frees it.**

- (a) A row's values are zeroed when the row is dropped, replaced, evicted, expired or cleared.

- (b) A name `lade ttl forget` removes from a row that keeps other names is zeroed too.

"The cap is a fence for a client that puts faster than TTL" (`docs/cache.md`, What the hub holds): the hub serves every directory and every repo of the user, and its RAM must stay bounded whatever a client sends. Past the bound the oldest insert goes, live or not (`tables_upsert_at_cap_keeps_row`). The bound itself, 256 rows, is a measurement, not the promise.

**§ <a id="gn0zrhfpq">gn0zrhfpq</a> The hub holds a bounded number of secret rows, and past the bound it drops the oldest insert, live or not.**
