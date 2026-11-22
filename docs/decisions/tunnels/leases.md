# Sharing a tunnel

Two commands to one target should share one provider, and a tunnel must stop once no command uses it, even when a command dies without saying so or fails on another tunnel. This note binds when a command reuses a tunnel, how the hub knows who holds one, and what a failed tunnel releases.

## Reuse

"The next command to the same target reuses it, with or without a ttl of its own" (`README.md`, Tunnel), and "two commands on the same target share one provider" (CHANGELOG 0.20.0, Every tunnel runs in the hub). Reuse is what makes a ttl worth having, and what makes two terminals on one database cost one `kubectl`. The target is the URI and the local host it listens on. On a port the rule names the port must match too. With an env-var key and no `local=` the hub also requires the same key ("an env-var key without `local=`. Reuse matches on `key`", `src/hub/wire.rs`), a rule of the code no doc states, so the law leaves that case out. On reuse "the ttl becomes the longer one" (`docs/tunnels.md`): one command's short ttl never cuts another's longer one. The model check in `src/hub/tunnels/model_tests.rs` asserts after every event that no reply hands out a tunnel to another target (`every_event_sequence_keeps_the_invariants`, `long_random_walks_keep_the_invariants`).

**§ <a id="gab93p2gf">gab93p2gf</a> A command reuses a running tunnel only when it goes to the command's own target, and on a port its rule names it reuses that tunnel instead of starting another.**

- (a) Two asks reach the same target only when they name the same URI and local host, and, on a port the rule names, the same port.

- (b) Reuse adds the command's lease and keeps the longer of the tunnel's ttl and the command's, also when the command has none.

## Leases

A tunnel lives while some command uses it, so the hub must know who uses it, and must notice when a user dies without saying so. A lease is "who keeps a tunnel open: `id` is a ticket or `p<pid>`. `started` tells a live `pid` from a reused one" (`src/hub/wire.rs`). From the `docs/tunnels.md` table: `lade unset` releases the ticket's lease, and a "leasing process gone (pid and start time)" loses its lease, "checked every second". A pid alone can be reused by the kernel and would keep a tunnel forever.

Pre-exec cannot lease to the command's process, which the shell starts after `lade set` exits. "`lade set` opens pre-exec tunnels under its own lease, so a Ctrl-C while they connect leaves nothing behind. Right before it prints, the terminal session leader's lease takes over, since `lade set` exits before the command runs. A `lade --` lease belongs to that process" (`docs/tunnels.md`). The session leader, not the parent, because "its parent is often a short-lived subshell" (`src/network/lease.rs`). Before 0.20.0 "a pre-exec tunnel stopped only when `lade unset` ran, so closing the terminal mid-command left it running. The hub now stops it within a second of the shell exiting, unless a ttl keeps it" (CHANGELOG 0.20.0, Tunnel cleanup). `lade unset` releasing the ticket's lease is [gy20cgb72.b](../wrap/tickets.md#gy20cgb72). The hub checks leases on its own clock, also when no request comes (test `a_busy_hub_stops_a_tunnel_whose_holder_died`), every second today, a value with no stated reason.

A ttl "counts from the last command that used it" (`docs/cache.md`, What the hub holds), so a tunnel used all afternoon with `ttl: 1h` stays an hour after the last command, not an hour after the first. Tests: `a_ttl_keeps_the_tunnel_for_the_next_command_only_until_it_runs_out`, `without_a_ttl_the_tunnel_stops_with_its_last_lease`, `a_dead_shell_releases_its_lease`.

**§ <a id="gv81k6nf4">gv81k6nf4</a> The hub keeps a tunnel for the commands that hold a lease on it, and each lease names one live process.**

- (a) `lade --`, a pre-tool wrap and `lade mcp` lease a command's tunnels to their own process, and release the lease when they exit.

- (b) `lade set` leases pre-exec tunnels to itself while they connect, and right before it prints hands them to the terminal session leader under the ticket; a failed handover cancels the command.

- (c) The hub drops a lease once its process, named by pid and start time, is gone, checked on a fixed interval whether or not a request comes.

- (d) A tunnel's ttl counts from when its last lease went, and starts over once a command leases it again.

## All or none

A command that needs two tunnels and gets one does not run, [t7kcndtra](../promises.md#t7kcndtra), so the tunnel it got is waste, and a start still running for it would outlive the command. "When one tunnel failed, the others still starting outlived the command. They now stop at the first error" (CHANGELOG 0.20.0, Tunnel cleanup). Lade joins each of its workers before it exits, since "a worker still waiting on the hub would keep its lease open" (`src/network/acquire.rs`). Ctrl-C is the same stop from outside, [s6j22qtcr](../wrap/cancel.md#s6j22qtcr). The tunnels already up go with the command's lease, [gv81k6nf4.a](leases.md#gv81k6nf4).

The CHANGELOG line states no ttl caveat, so a ttl does not keep a start the command gave up on: the hub stops a start once its last lease goes, with a rule `ttl:` or a `lade ttl set` window as without. A ttl keeps a tunnel that came up for the next command ([gv81k6nf4.d](leases.md#gv81k6nf4)), so one that was ready before the failure stays for its ttl. Test: `a_start_its_command_gave_up_on_stops_despite_a_ttl`.

**§ <a id="kd0fkaw1x">kd0fkaw1x</a> At a command's first failed tunnel, Lade stops waiting for every other tunnel of that command and releases its lease on each of them.**

- (a) The hub stops a tunnel still starting once its last lease goes, whatever its ttl.
