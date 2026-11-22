# The wire

Every request to the hub crosses a socket, and the hub must never slow or break a command: a cache that costs more than the provider is worse than no cache. This note binds what a connection carries, how long a client waits and when it replaces a hub, and what the hub's copy of a ticket is worth.

## The wire

"One connection is one request, one response, then close" (`docs/cache.md`, Wire). The frame is short and checked before anything is decoded: "Magic `LDH1`, then `u32le` body length ... `N == 0`, `N > 1048576`, or bad magic: drop, no `Rep`" (`docs/cache.md`, Wire), so a stray process writing to the socket cannot make the hub allocate or parse at will. "Every frame also carries `CARGO_PKG_VERSION`" (`docs/cache.md`, Process): two binaries with the same image token cannot exist, but a request of another version is the sign of a hub left from before an upgrade, and the hub serves nothing of it (test `wire_other_bin_ver_is_denied`). The version sits in the same archive as the body, so the hub reads it only once the whole request decodes: a request of another version whose layout differs fails to decode and is dropped with no reply, which the client's ping then reads as stale ([cex82a6ad.a](wire.md#cex82a6ad)).

The hub serves one connection at a time, so its tables need no lock, "and drops one that has not finished after 500 ms" (`docs/cache.md`, Wire, Failures: "Client connects and sends nothing | Dropped after 500 ms, next client served"): one that stalls must not hold every other client, and the idle and reach checks, behind it (test `a_silent_client_does_not_hold_the_hub`). The 1 MiB frame cap and the 500 ms timeout are the docs' values, and no source gives a reason for either number, so the law names the bound and leaves the value to `docs/cache.md`. The same holds for the bytes of the header, the magic `LDH1` and the length as a little-endian `u32`: the reason given is a header checked before decoding, which any fixed magic and length prefix buy.

**§ <a id="dd89rj943">dd89rj943</a> A hub connection carries one framed request and its one reply, and the hub serves requests of its own version alone.**

- (a) A frame is a fixed magic, the body length, then the body, and the hub drops a frame whose length is 0 or over a fixed cap, or whose magic differs, without a reply.

- (b) Every request and reply carries the crate version: the hub answers a request of another version with `Err` 4 when it decodes and drops it without a reply when it does not, and a client reads a reply of another version as `Err` 4.

- (c) The hub serves one connection at a time, and drops a connection that has not finished within a fixed timeout.

## A client's wait

The cache is there to save time: "A cache hit costs one socket round trip: under 1 ms" (`docs/cache.md`, What the hub holds). A lookup that waited for a cold hub to start would cost every first command more than it saves, so it does not: "No full reply in 20 ms: miss, hydrate, at most one later `Put`. Ignore a late `Hit`" and "Spawn fail: hydrate as usual" (`docs/cache.md`, Process; Failures: "Socket > 20 ms | Miss, hydrate, one `Put`"; "Hub dies mid-`Put` | This command uses its hydrate"). A request a command cannot do without, a tunnel or a window, waits longer for a cold hub (`src/hub/transport.rs`), and [y8w7xhrr1.e](../cache/ttl.md#y8w7xhrr1) waits for the value the user asked to keep; what a command does without a hub for its tunnels is `docs/tunnels.md`'s.

The deadline is 20 ms, and a lookup whose request was written with no reply then pings the hub for up to 1 s before it calls the hub stale (`docs/cache.md`, Process and Failures). A hub that takes a request and never answers therefore costs a lookup about a second, not 20 ms. No source gives a reason for either number, so the law names the deadline and not its value. A stale hub is replaced on "a fresh 20 ms clock" (`docs/cache.md`, Process), so a lookup that met one waits up to two deadlines and the ping. The start lock is under the deadline too: a client that finds another one starting the hub waits for its lock only until the deadline, then misses (`a_lookup_misses_in_time_while_another_client_holds_the_start_lock`).

**§ <a id="e9ee1e4md">e9ee1e4md</a> A cache lookup gives the hub a short deadline, and a hub that is slow, down or cannot start costs the command a miss.**

- (a) The deadline covers connecting, the start lock, starting a cold hub and reading the full reply, and a later reply is ignored; past it, a lookup waits only for the stale check of [cex82a6ad.a](wire.md#cex82a6ad) and the one retry on a fresh deadline of [cex82a6ad.b](wire.md#cex82a6ad).

- (b) A lookup with no reply in time is a miss, and the command hydrates from its providers as if no hub ran.

A hub can be stale: one of another version after an upgrade, or one that no longer reads its socket. A client that may start a hub replaces a stale one once: "Mismatch, or a successful write with no decodable `Rep`: the client unlinks its own socket and spawns once on a fresh 20 ms clock. It does not touch another image's hub" (`docs/cache.md`, Process). A busy hub is not a stale one, and replacing it would stop the tunnels of every command it serves: "A client whose request times out pings the hub for a second before calling it stale: replacing a busy hub would stop its tunnels" (`docs/tunnels.md`; test `a_slow_hub_is_not_stale_and_a_wedged_one_is`). A client that may not start a hub, such as `lade status`, only reports a denied hub as `stale`. A socket that refuses a connect has no hub behind it: the client removes it before it starts one, which replaces no hub (`src/hub/transport.rs`, `connect`).

The ping waits 1 s (`docs/cache.md`, Process), a value no source gives a reason for, so the law names a fixed wait. It tells a busy hub from a wedged one only within that wait: a hub busy past it, such as one serving two silent clients queued ahead of the ping, reads as stale and is replaced, which stops its tunnels.

**§ <a id="cex82a6ad">cex82a6ad</a> A client replaces its image's hub only when that hub denies it, or leaves a request unanswered and then a ping too.**

- (a) A hub is stale when it answers `Err` 4, or when a request got no decodable reply and the hub then answers no `Ping` within a fixed wait.

- (b) A client allowed to start a hub replaces a stale one: it unlinks that socket, starts a new hub, and sends its request once more on a fresh deadline.

## The hub's ticket table

"T has no secret values. The JSON file is still the peel ... Wrap / `unset` may `PutT` / `UnlinkT` after. Hub T is optional. `ticket_ready` and space-form `--pretool id` stay file-only" (`docs/cache.md`, Tickets; Failures: "Hub dies, leftover T files | Files still work"). The file is the ticket ([nv6nfbbs2](../wrap/tickets.md#nv6nfbbs2)); the hub's copy keeps the hub up while a pre-exec command runs, and its first `PutT` carries the notice request ([jpm1fzdj4.a](notices.md#jpm1fzdj4)). A hub that is down, stale or lost must therefore change nothing about a command. The table is fenced like the secret table, against a client that puts faster than it unlinks: "Tickets | 128 rows | `UnlinkT`, or older than 1 h" (`docs/cache.md`, What the hub holds). No source gives a reason for 128 or for an hour, so the law names a fixed count and age.

**§ <a id="spzvjnk1e">spzvjnk1e</a> Every reader of a ticket reads its file, and the hub holds a best-effort copy of it.**

- (a) Sending a ticket to the hub, or removing it there, is best effort: the command runs and ends the same whether it succeeds or fails.

- (b) The hub drops a ticket on `UnlinkT`, a fixed age after its `PutT`, and the oldest first past a fixed count of tickets.
