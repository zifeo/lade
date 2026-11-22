# Tunnels and their ports

A rule can open a private network for one command. `DB_PORT: kubectl://...` or `1223: ssh://jump.example.com/db.internal/5432` gives the command a local port that forwards to a database, a service or an app behind a cluster, a bastion or a Teleport proxy (`README.md`, Tunnel; Patterns, Private networks: "Open a local forward only while the command runs, then close it automatically"). Lade does not carry the bytes to the target itself. It runs a provider CLI, `kubectl port-forward`, `kubefwd`, `tsh` or `ssh`, and relays the local port to wherever that provider listens.

The problem is the local port. Every program on the machine shares the same port numbers, a command reaches whatever listens on its port, and the provider is a third-party process with its own ideas about binding, prompting and dying. The bug that shaped this design: "two commands on one fixed local port shared the first tunnel. The second reported ready by connecting to the first, and reached the first one's target, for example a stg apply reaching the prd database" (CHANGELOG 0.20.0, Tunnel port clash). The project names other failures in the same release: a pre-exec tunnel left running after its terminal closed, a provider whose helpers ignored `SIGTERM`, a provider orphaned when Lade was killed, and a provider that died an hour in and was never restarted (CHANGELOG 0.20.0, Tunnel cleanup and Tunnel restarts).

The promise is in [jfxr4rmae.c](../promises.md#jfxr4rmae): a tunnel stops once no running command uses it, unless a ttl keeps it. A failed tunnel cancels the command, [t7kcndtra](../promises.md#t7kcndtra), and Ctrl-C while tunnels connect stops them, [s6j22qtcr](../wrap/cancel.md#s6j22qtcr). How long a ttl may be, and the user's `max_cache` ceiling over it, are [cfan04ksd](../cache/ttl.md#cfan04ksd). `lade ttl forget` stopping tunnels is [wszszmyqk](../cache/ttl.md#wszszmyqk). The hub process itself, its socket, who may talk to it and when it exits, is the hub note's. These notes state how a tunnel claims its port and waits or is refused, here, how it is shared and held, [leases](leases.md), and how it is restarted and stopped and what Lade asks of each provider, [providers](providers.md).

## One place for every tunnel

"The hub that caches secrets runs every tunnel, from pre-exec, `lade --`, and harnesses alike, so one place knows every Lade tunnel on a port. Without a hub there is no tunnel: the command fails like any other acquisition" (`docs/tunnels.md`). 0.20.0 moved every tunnel there, "for pre-exec, inject, pre-tool, and MCP alike", and "a command fails when the hub cannot start" (CHANGELOG 0.20.0, Every tunnel runs in the hub). A fallback that ran a tunnel in the command's own process would be a tunnel the hub cannot see, and the next command on that port could not tell a Lade tunnel it should wait for from a stranger it should refuse. Test: `without_a_hub_a_command_with_a_tunnel_does_not_run`.

**§ <a id="r14dqggh2">r14dqggh2</a> Every tunnel runs in the hub of the Lade binary that asks for it, and a command whose hub does not answer fails.**

Each Lade binary has its own hub (`docs/tunnels.md`; [keyfb5k7w](../promises.md#keyfb5k7w)), so a tunnel of another Lade binary is, to this one, any other program on the port.

## The local port

"An env-var key gets an ephemeral local port unless `local=` sets one. A numeric key is a fixed port" (`README.md`, Tunnel). A fixed port serves a client that cannot read an env var, such as `psql -p 1223` in the README example. An env var is the safe choice, since the port Lade picks is free. A key and a `local=` that name two ports have no right answer, so the binding fails. Port 0 would let the kernel pick a port while the tunnel records 0, so a key `0`, or a `local=` port 0, fails the binding too (`a_port_zero_key_or_local_fails_the_binding`).

The `tsh://` app proxy stays on loopback: "Loopback local binds stay on `127.0.0.1`" (CHANGELOG 0.17.0, Teleport `tsh` URIs; test `parse_binding_keeps_tsh_app_proxies_on_loopback`). The docs state the value only. The reason is the code's own comment: the proxy "speaks with the user's Teleport identity" (`src/network/parse.rs`), so whoever reaches its port is the user, to that app.

**§ <a id="q6k9k1zr8">q6k9k1zr8</a> A tunnel's key decides its local port: a number is that fixed port, and an env var name gets a port Lade exports in that variable.**

- (a) A numeric key from 1 to 65535 listens on that port, and a key `0` or a `local=` naming another port fails the binding.

- (b) An env-var key listens on the port its `local=` names, or else on a free port Lade picks, and the command gets that port in the variable.

- (c) A `tsh://` app tunnel listens only on `127.0.0.1` or `localhost`.

## Holding the port is the claim

"Lade binds the local port itself and relays each connection to the provider ... Holding the port is the claim. A second tunnel on that port gets `AddrInUse` at once, from this hub, another hub, or any other process" (`docs/tunnels.md`). Before 0.20.0 the provider bound the port, and readiness was a connect to it, which succeeded against the first command's tunnel (CHANGELOG 0.20.0, Tunnel port clash). Binding first makes the kernel the arbiter: "The kernel frees the port when its holder dies, so there is no lock file to clean up. A provider restart keeps the port: nothing else can take it in between" (`docs/tunnels.md`). A lock file would outlive a crash and need a sweeper. A bound socket cannot outlive its holder.

macOS lets `127.0.0.1:P` bind while `0.0.0.0:P` listens, and a `localhost` client may reach either, "so a fixed port also counts as held when `0.0.0.0`, `::`, `127.0.0.1`, or `::1` is taken on it" (`docs/tunnels.md`; tests `loopback_claim_is_refused_while_a_wildcard_listens`, `wildcard_claim_is_refused_while_loopback_listens`). A port the kernel picks only avoids the identical address, so Lade checks a picked port the same way (`src/network/outer.rs`). No doc states that half, so the law covers a fixed port only. The check binds each address with `SO_REUSEPORT`, on purpose, so two claims probing one port at once pass each other (`src/network/outer.rs`, `probe`). A holder of the same user that also set `SO_REUSEPORT` lets that bind through, so it does not count as holding the port: a stated limit.

A port another program holds is not Lade's to wait for: "A port another program holds, on any loopback or wildcard address, is refused with a box right away" (`README.md`, Tunnel), since "the command would reach that listener instead" (`src/network/labels.rs`; tests `test_inject_network_refuses_a_port_already_in_use`, and `zsh_does_not_run_a_command_whose_port_is_held` with its bash and fish twins). "Each Lade binary has its own hub, so a tunnel of another Lade binary on that port looks like any other program: refused, not waited for" (`docs/tunnels.md`).

A stopped tunnel gives its port back before the hub answers anyone else. Otherwise the command waiting for it would find the port still taken and be refused as if a stranger held it. The provider may take seconds to die, so a relayed provider stops off the request loop, after the port is free (`docs/tunnels.md`; test `a_released_tunnel_frees_its_port_before_release_returns`). kubefwd holds the port itself, so it is stopped first, on the request loop. A tunnel still starting is the exception, below.

**§ <a id="ewd09vh5j">ewd09vh5j</a> Lade binds a tunnel's local port itself, before it starts the provider, and holding that port is the tunnel's only claim to it.**

- (a) The hub keeps the port bound for the tunnel's whole life, across every restart of its provider, and relays each connection to wherever the provider listens now.

- (b) A fixed port counts as held when a socket that does not share it through `SO_REUSEPORT` is bound to `0.0.0.0`, `::`, `127.0.0.1` or `::1` on it.

- (c) A port held by anything other than this hub's own tunnels and starts, a tunnel of another Lade binary included, fails the command at once, naming the port.

- (d) Stopping a ready tunnel frees its port before the hub answers another request.

kubefwd is the exception to (a), stated in [jfg5ews57.c](providers.md#jfg5ews57).

## Waiting for another target

Two commands on one fixed port with two targets cannot share a tunnel, and refusing the second at once would fail every terminal that runs the same client against another environment a minute later. "When a command uses another Lade tunnel to a different target on that fixed port, the new command waits up to 10 min for it to end, then gives up with a box. A tunnel kept only by its ttl gives the port up at once" (`README.md`, Tunnel). The table in `docs/tunnels.md` gives the hub's side: a holder "in use or starting" refuses, "naming the holder's key and URI", and "the client asks again every 500 ms for up to 10 min"; an idle holder kept only by its ttl is stopped and the new tunnel starts on its port. A ttl is a convenience for the next command to the same target, never a reason to block a command that needs the port now. The box after the wait names the holder and how to free the port, `lade ttl forget <key>` or an env-var key (`src/network/labels.rs`). Tests: `a_second_target_waits_for_the_first_command_to_end`, `an_idle_tunnel_gives_its_port_to_another_target`, `prd_then_stg_on_one_port_is_refused`, `network_k3d_kubectl_provider_lifecycle`.

The 500 ms poll has no stated reason and stays out of the law. The wait is bounded at 10 minutes, the README's figure, which no source argues for, so the law binds a bounded wait and the number stays here.

A start the hub cancelled frees its port only when its start thread ends. A command asking for that port meanwhile meets this hub's own start, not a stranger, so it waits instead of failing (`src/hub/tunnels/mod.rs`; test `a_cancelled_start_makes_the_next_ask_wait_instead_of_failing`). That is the "this hub's own ... starts" of [ewd09vh5j.c](ports.md#ewd09vh5j).

**§ <a id="vpxx2dd6c">vpxx2dd6c</a> A command whose fixed port holds this hub's tunnel to another target waits a bounded time for that tunnel to end, then gets a tunnel to its own target or fails.**

- (a) While a command uses the holder, or the holder is still starting, the hub refuses the ask and names the holder's key and URI.

- (b) A ready holder that no command uses, kept only by its ttl, stops at once and its port goes to the asking command.

- (c) The asking command asks again until the holder is gone or the bound has passed, then fails naming the holder and how to free the port.
