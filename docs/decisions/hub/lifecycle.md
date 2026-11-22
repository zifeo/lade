# Finding, starting and ending a hub

A client must reach the right hub, a hub must start only when a command needs it, and it must end when nothing does: a hub left behind by an upgrade serves the old binary's tunnels, and one started for a plain command costs it time for nothing. This note binds which hub a client reaches, where its socket lives, when it ends, and which commands talk to it.

## One hub per binary image

"Each image has its own socket, so several hubs can be up ... Cache does not cross binaries" (`README.md`, Development). A user may run `cargo run` next to an installed `lade`, or a mise-pinned version next to another, all at one crate version. Values must not cross between them, since each checks that its peer is its own binary ([e1s6kxps0](security.md#e1s6kxps0)), and a stale hub of one must not be mistaken for the other's. So the socket is named by the image: "`token` is 16 hex chars of SHA-256 over this process's mapped image (`dev`, `ino`, size, mtime, first 4 KiB). Same crate version, different file: different socket" (`docs/cache.md`, Process). `lade status` and `lade log prune --hub` "talk to this image only", and "Other `hub-*.sock` leftovers stay" (`docs/cache.md`, Process and CLI).

The client starts the hub itself when it is needed: "Socket absent: `flock` `$CACHE/hub-{token}.lock`, spawn `lade __hub`, exclusive `bind`. The bind loser connects once to the winner" (`docs/cache.md`, Process). It starts its own executable, so the hub is the same binary the peer check expects, in its own session with no stdin, so the shell's job control and its terminal do not reach it. "A binary not named `lade` runs no tunnel" (CHANGELOG 0.20.0, Every tunnel runs in the hub), and "a release Lade starts its hub from its own executable, whatever the environment holds" (CHANGELOG, Unreleased). A debug build, the one `cargo test` runs, takes the hub from `CARGO_BIN_EXE_lade` when that is set, since a test harness is not named `lade` (`src/hub/transport.rs`). An image that cannot read its own file has no token to name its socket, and sharing a fallback name with other images would cross binaries, so it reaches no hub at all.

One socket per image is not one process per image: after a client replaces a stale hub ([cex82a6ad](wire.md#cex82a6ad)), the old one runs on, unreachable, until its next reach check, up to a minute ([cbz2he75h.b](lifecycle.md#cbz2he75h)).

**§ <a id="hxw00jp6v">hxw00jp6v</a> Each Lade binary image has a hub socket of its own, and a client reaches only the hub of the image it runs.**

- (a) The socket is `hub-<token>.sock` in Lade's cache root, the token naming the running image's file, so two files of one build reach two hubs, and an image that cannot read its own file runs as with a hub that cannot start.

- (b) A release client starts a hub only from its own executable, and only when that is named `lade`, as `lade __hub` in a new session with no stdin.

- (c) A client starts a hub under an exclusive lock on the image's lock file, the hub binds its socket exclusively, and a hub that loses the bind exits at once.

- (d) The socket is created with mode `0600`, and a hub that cannot make it so removes the socket and exits.

The socket is the door to plaintext values, [keyfb5k7w](../promises.md#keyfb5k7w), so the mode keeps another uid off it before the peer check runs, a guard in depth beside [e1s6kxps0.a](security.md#e1s6kxps0) (CHANGELOG, Unreleased, The hub socket's mode). The ssh control socket's directory, `0700` in `src/network/tunnel.rs`, is no law by the same measure: the port the tunnel forwards listens on loopback, open to every local user, so the directory's mode guards nothing a law promises.

## The cache root

The socket, the tickets and the mise isolation live in Lade's cache root: `LADE_CACHE_DIR` when it is set, else the OS cache directory under the user's home (`docs/env.md`, User overrides; `src/cache.rs`). Before 0.20.0, Lade with no home cached in `.lade-cache` under the current directory; it now "stops Lade with a box" (CHANGELOG 0.20.0, Upgrading: "No home directory is an error. Set `HOME`, or `LADE_CACHE_DIR`"), since tickets and sockets in the working tree would land in the repo. The sources state a box, not an exit code, so the law binds an error box. The space form `--pretool <id>` reads argv before that check, and finds a ticket only once the cache root resolves, so it too reaches the box. No test reaches the case: with `HOME` unset the OS still reads the home from the user database.

**§ <a id="qnn5xnx4z">qnn5xnx4z</a> With neither `LADE_CACHE_DIR` nor a home directory, Lade fails with an error box before any verb runs.**

`--help` and `--version` answer before that check: they cache nothing.

## When a hub ends

A hub has nothing to keep once its tables are empty, and an hour with no request is the sign no command needs it: "After 1h with no request, it prunes expired rows. It exits when no secret, ticket, tunnel, or `lade ttl set` window is left. A window keeps it up until `lade ttl unset`" (`docs/cache.md`, Process). A window is the user's own request and does not expire on its own ([zdyc7ggsy.d](../cache/ttl.md#zdyc7ggsy)), so it holds the hub.

A hub its clients can no longer reach must not wait out the hour holding secrets and tunnels nobody can ask for. A reinstall or `lade upgrade` puts another file at the binary path, new clients hash another token, and "The old hub sees its binary replaced and exits within a minute" (`docs/cache.md`, Process; `README.md`, Development; CHANGELOG 0.20.0: "A reinstall or `lade upgrade` stops the old hub within a minute, and the tunnels of commands still running with it"). The same holds when a client replaced the socket with a fresh hub's. On exit the hub removes its socket and lock "only if the socket path is still its own": another hub may already be bound there (`docs/cache.md`, Process; tests `orphan_hub_keeps_the_socket_that_replaced_it`, `idle_empty_hub_exits_and_removes_its_socket`). The check runs on its own clock: a stream of requests from old clients must not keep an orphan up past the minute. The idle time is an hour and the bound a minute: the sources state both and argue for neither value, so the law binds a fixed idle time and a fixed bound.

Nothing in a hub is written down: "Crash is always cold. Nothing is serialized" (`docs/cache.md`, Process), and the version ledger is "RAM only, like the rest. A new hub checks again" (`docs/cache.md`, Who talks to it). Every row it holds can be rebuilt: a secret by its provider, a ticket from its file ([spzvjnk1e](wire.md#spzvjnk1e)), the ledger by the next check.

**§ <a id="cbz2he75h">cbz2he75h</a> A hub exits once it holds nothing and has had no request for a fixed idle time, or within a fixed bound once its clients can no longer reach it.**

- (a) After the idle time with no request, the hub exits when no secret, ticket, tunnel or window is left, and stays up otherwise.

- (b) At least once per bound, whatever requests arrive, the hub checks that its socket path still holds its own socket and its binary path its own file, and exits when either fails, rows or not.

- (c) On exit the hub removes its socket and its lock only when the socket path still holds its own socket.

- (d) A hub starts with empty tables and an empty release ledger, and holds them in its memory alone, so they end with it.

- (e) A link the hub holds for `lade connect` keeps it up as a tunnel does, until the link stops or fails, [dzex6kwmq](../remote.md#dzex6kwmq).

## Who talks to it

A command that has nothing for the hub must not pay for it: "A plain `set` (no secret, tunnel, disclaimer, or pinned tool) never connects" (`docs/cache.md`, Who talks to it), and "a plain command stays off the hub" (`AGENTS.md`, House rules, Shell notices; `docs/architecture.md`: "A plain command never calls the hub"). `set`, `inject`, the pre-tool wrap and `mcp` connect for a matched command's secrets, tunnels, ticket and notices, and may start the hub. Commands that only look at the hub or stop it never start one: "`lade status` Pings a live hub and reads its ledger. `setup`, `update`, and `upgrade` read the ledger for the release line. `lade log prune --hub` sends `Quit`. None of them spawns", and `lade ttl` list and forget "do not spawn" (`docs/cache.md`, Who talks to it and CLI). Starting a hub to report that none ran, or to stop it, would leave one behind. `unset` "may `UnlinkT` only", and releases its tunnels: a hub that is down holds neither.

A rule that only cancels keys hands the hub nothing, so `lade set` and the pre-tool wrap both run it with no ticket sent to the hub and no notice slot asked (`a_pretool_wrap_that_only_cancels_keys_starts_no_hub`, `a_preexec_set_that_only_cancels_keys_starts_no_hub`). A line that replaces the pending ticket a withheld disclaimer left in `LADE_T` deletes the ticket's file and leaves the hub's copy to the hub's own expiry, [spzvjnk1e.b](wire.md#spzvjnk1e), since readers read the file, so a cancel-only line has no reason to connect. Some doors never connect at all: `age-plugin-lade`, `on`, `off`, `bench`, and the pre-tool handler, `lade hook` on stdin, which "writes the T file only" so a harness's hook stays fast; `lade eval` without `--ttl` is [xrjs58t2q.b](../secrets/eval.md#xrjs58t2q). `CI` in the environment changes none of this (`docs/cache.md`, Who talks to it: "`CI` is not a signal").

**§ <a id="f18xcggkd">f18xcggkd</a> A command connects to the hub only for work the hub does for it, and only a command that hands the hub such work starts one.**

- (a) A command connects to the hub, through any door, only when it has a secret, tunnel, disclaimer or pinned tool to prepare.

- (b) `lade unset` removes its ticket from a hub already up and releases its tunnels there.

- (c) `lade status`, `setup`, `update`, `upgrade`'s release line, `lade ttl` list and forget, and `lade log prune --hub` only talk to a hub already up.
