# How long a value stays

A value kept too long outlives a rotation in the vault, and one kept too briefly asks for Touch ID on every command. The repo, the user in a directory, and the user for every repo each have a say, and the user's word caps the others. This note binds the ttl of a cached value, the window a directory may set, the user's ceiling, and how to drop everything now.

## How long

The default is 5 minutes "so a Touch ID or key touch is not asked again in that time" (`README.md`, Secret; CHANGELOG 0.20.0, Secret cache: "agents included"). `ttl:` on the body turns it off or sets another duration. `off` is how a repo says one rule must always fetch fresh, such as an API token (`README.md`, Secret, the `^curl` example), and it holds even when the user marks the directory with a window (`docs/cache.md`, Opt in: "`ttl: off` on the body still wins"). The README bounds a ttl at 24 hours in bold, and a longer one fails at load rather than being shortened (`docs/cache.md`, Opt in; `ttl_parse_caps_at_24h`): a repo that asks for more than Lade will keep must hear it, not get a shorter cache in silence. CHANGELOG 0.20.0 also fixed a panic on a huge `ttl:` under Panics. The same parser reads `lade eval --ttl`, a window and `max_cache`. The sources give a reason to cache and a reason to fail rather than shorten, but none for 5 minutes or 24 hours themselves, so the default is 5 minutes and the bound 24 hours here, in prose, and the law binds only that both are fixed.

**§ <a id="fkaa6vap4">fkaa6vap4</a> A cacheable body keeps its values in the hub for a fixed default unless its `ttl:` says otherwise, up to a fixed bound.**

- (a) With no `ttl:` and no window, a cacheable body's values stay for the default.

- (b) `ttl:` on the body's `.` block is `off` or a duration, and `off` turns the cache off for that body, even under a window.

- (c) A ttl past the bound fails where it is read, the load of a `lade.yaml` included.

A cached value lives from its fetch: "stays in RAM for 5 minutes after the first hydrate" (`README.md`, Secret), and for `lade eval --ttl` "A hit does not refresh the clock" (`docs/cache.md`). A clock a hit restarted would keep a value as long as commands keep using it, which is the opposite of a ttl. Only a new fetch, or a window the user sets, starts the clock again.

The clock "keeps counting while the machine sleeps (`CLOCK_MONOTONIC` on macOS, `CLOCK_BOOTTIME` on Linux), so a closed lid does not stretch a TTL" (`docs/cache.md`, What the hub holds; CHANGELOG 0.20.0): `Instant` stops during sleep on both systems, so a 5 minute value left overnight would still be served in the morning. Every request first drops the rows that are due, so a due row is never served, even by a hub that slept through its idle prune.

A provider's new value does not reach a command while a row is live: "A vault edit does not evict RAM" (`README.md`, Commands; `docs/cache.md`, Failures, "Provider value changed, hub still live"). That is the price of not asking the provider; `lade ttl forget` is the way out.

**§ <a id="hadg0brg4">hadg0brg4</a> A cached value is served for its ttl from when it was fetched, and expires when that ttl ends.**

- (a) Only a new fetch or a window restarts a value's clock, and a value served from the hub keeps its clock running.

- (b) The clock keeps counting while the machine sleeps.

- (c) Every request to the hub first drops the rows whose ttl is over.

## A window for a directory

A user who works on one repo for an afternoon wants fewer prompts there without editing a shared `lade.yaml`. `lade ttl set 2h` marks this directory on the hub, and the window then replaces the ttl of every cacheable body run there, shell ones included, until `lade ttl unset` (`README.md`, Secret and Commands; `docs/cache.md`, Opt in). The body's `ttl: off` still wins ([fkaa6vap4.b](ttl.md#fkaa6vap4)). A window is the user's own request, so it does not expire on its own (`src/hub/tables.rs`: "they are a `lade ttl set` the user asked for").

A window covers the directory, "Ancestors and children share it" (`docs/cache.md`, Opt in; CHANGELOG 0.20.0: "marks a window on this cwd and its parents and children"), and "nearest path wins" (`docs/cache.md`, CLI), so in a subfolder its own window beats the repo root's, and at the repo root the root's beats a subfolder's. Distance counts path components. `/` follows the same rule as any directory, so a window set there covers every directory, and a command run there shares its children's (`a_window_in_the_root_covers_every_directory_and_the_root_shares_a_childs`). The docs leave a tie open, and a hub that picked by its own storage order gave one answer one day and another the next, so the tie is settled in a fixed order. The order itself is Lade's pick, not a source's: a parent covers the directory where a child only shares it, so the parent goes first, and two children go by the byte order of their paths (`the_nearest_window_wins_and_a_tie_is_settled_by_path`). Setting a window restarts the rows already cached there with its ttl (`docs/cache.md`, Max cache: "a window restamps at most to it"; `tables_window_marks_scope_and_restamps`), so the value the user just fetched does not expire 5 minutes later. It restarts only the rows whose nearest window it is: a row under a nearer window keeps that window's ttl (`a_window_restamps_only_the_rows_it_governs`). A `lade eval --ttl` row is not a rule body's and keeps the flag's ttl, [y8w7xhrr1.d](ttl.md#y8w7xhrr1) (`window_leaves_eval_rows_alone`).

**§ <a id="zdyc7ggsy">zdyc7ggsy</a> `lade ttl set <ttl>` makes that ttl the ttl of every cacheable body run under the marked directory, until `lade ttl unset`.**

- (a) The window replaces a body's `ttl:` duration and the default.

- (b) A window applies in the marked directory, its ancestors and its descendants, and where several apply, the nearest wins, the fewest path components away, and a tie settles in a fixed order.

- (c) Setting a window restarts, with its ttl, the clock of each rule body's row already cached in a directory where it is the window that applies.

- (d) A window lasts until it is cleared, and `lade ttl unset` clears the window of this directory only.

## The user's ceiling

A `lade.yaml` is the repo's word; the user may want less kept on their machine whatever any repo says. `max_cache` in `config.json` is that ceiling, "for this user", over every secret and tunnel ttl and every window: `"1h"`, or `"0"` to turn the cache off (`README.md`, Secret; `docs/cache.md`, Max cache; CHANGELOG 0.20.0, `max_cache`). "The hub enforces it, not the client" (`docs/cache.md`, Max cache): the client is the code that reads the repo's yaml, and the ceiling must hold whatever ttl a client asks. Reading `config.json` and keeping `max_cache` raw so a bad value never stops `user` from loading is [pq5ebbcs0](../config/discovery.md#pq5ebbcs0).

"Any change, up or down, drops every cached secret and stops every tunnel no command holds, so a raised cap never extends what is already there" (`README.md`, Secret; `any_max_cache_change_drops_cached_secrets`). A user who lowers the ceiling expects the RAM to shrink now, and one who raises it must not find that rows cached under the old ceiling now live longer. Tunnels a running command holds stay, capped. An unreadable file or an invalid value counts as `"0"` (`docs/cache.md`, Max cache; `an_invalid_max_cache_keeps_nothing`): a user who typed the ceiling wrong asked for less, never for none at all. A missing field, or a missing file, sets no ceiling.

The hub watches the file and applies a change while it runs; a thread does the reading so a request never waits on the file system (`src/hub/cap.rs`; `docs/cache.md`, Max cache). `lade status` shows the ceiling in force and a window it shortens, and an invalid value sets `ok` to false (`docs/cli.md`, `lade status`; `test_status_fails_on_an_invalid_max_cache`); a `config.json` it cannot read or parse shows as cache off with its error, as the hub reads it (`status_shows_the_ceiling_of_a_config_json_it_cannot_parse`), which is [vk7cw8j73.a](../observability/status.md#vk7cw8j73).

**§ <a id="cfan04ksd">cfan04ksd</a> `max_cache` in `config.json` is the user's ceiling on how long the hub keeps a secret, a window's rows or an idle tunnel, and the hub enforces it.**

- (a) The hub shortens every secret row, window restart and tunnel ttl to the ceiling, and `0` (`"0"`, `0` or `"off"`) turns the secret cache off.

- (b) Any change of the value, up, down or removed, drops every cached secret and stops every tunnel no command holds.

- (c) An unreadable `config.json` or an invalid value, a duration past the ttl bound included, counts as `0` until fixed, and with a missing file or field the ttl bound alone applies.

- (d) The hub applies an edit of `config.json` while it runs, without a restart.

- (e) `lade status` shows the ceiling in force.

## Dropping it now

"`lade ttl forget` drops the RAM and stops the tunnels now" (`README.md`, Secret), "`lade ttl forget KEY` drops that secret name or tunnel key" (`docs/cli.md`, `lade ttl`), and "`lade log prune --hub` is an alias of `lade ttl forget`" (`docs/cache.md`, CLI). This is how a user who rotated a credential, or who is about to hand the laptop over, takes the copies back at once instead of waiting for the ttl. With no name it stops the hub, so nothing it held survives, windows included (CHANGELOG 0.20.0: "With no key it stops the hub and every tunnel").

**§ <a id="wszszmyqk">wszszmyqk</a> `lade ttl forget` drops cached values at once, and the next matching command fetches them again.**

- (a) With no name, it stops this binary's hub, which drops every row, window and tunnel, and `lade log prune --hub` does the same.

- (b) With names, it drops each named secret from every row of every directory, and the tunnels with that key.

## One URI with a ttl

`lade eval --ttl 5m <uri>` keeps one value for scripts and `age-plugin-lade` "like a rule body's `ttl:`: same duration format, at most 24h, `off` caches nothing, `max_cache` caps it, and a shell URI is cached since the flag opts it in. A raw, tunnel, or package URI fails" (`docs/cache.md`, `lade eval --ttl`; CHANGELOG 0.20.1). The flag is an explicit request, so a URI that can never be cached is an error, not a silent fetch. The row lives outside any yaml body: its key is the directory, the URI and the Lade user, and the URI is also the row's name, "so `lade ttl` lists it and `lade ttl forget <uri>` drops it". "A `lade ttl set` window does not change it: the flag is the ttl" (`docs/cache.md`). Since the user asked for the value to stay, eval waits for a cold hub to store it, up to 2 s (`docs/cache.md`, Who talks to it; `src/hub/apply.rs`), where a wrap gives a hub 20 ms and moves on. Without `--ttl`, eval never touches the hub, [xrjs58t2q.b](../secrets/eval.md#xrjs58t2q).

**§ <a id="y8w7xhrr1">y8w7xhrr1</a> `lade eval --ttl <ttl> <uri>` keeps that one value in the hub like a rule body's `ttl:`.**

- (a) The flag takes a rule's `ttl:` format.

- (b) With a duration, a shell URI is cached, and a raw, tunnel or package URI fails.

- (c) Its value reaches only a `lade eval --ttl` of the same URI by the same Lade user in the same directory, and `lade ttl` lists it and `lade ttl forget` drops it under the URI.

- (d) Its ttl is the flag's, whatever `lade ttl set` window applies.

- (e) It waits a bounded time for a cold hub to store the value.
