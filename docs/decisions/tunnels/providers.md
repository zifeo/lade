# Tunnel providers

A provider is a third-party process with its own ideas about binding, prompting and dying, and it runs in a hub that has no terminal. This note binds where a provider is prepared and runs, where it listens, when its tunnel is ready, how it is restarted, and that it never outlives its Lade.

## Launching a provider

The hub starts the provider, but the hub has no terminal and is not the user's shell. "A tunnel provider has no terminal, so it cannot ask for a password or a code: authenticate it beforehand (an ssh key or agent, `tsh login`, a kubeconfig that needs no prompt)" (`docs/tunnels.md`). What may prompt runs in the command's own Lade process, where the terminal is ("`tsh kube login` may prompt", `src/network/binding.rs`), and stops on Ctrl-C: "so does a `tsh kube login` still waiting" (CHANGELOG 0.20.0, Tunnel cleanup).

The kube context and the env come from the same place: "Built where the user's env lives: the kube context comes from their kubeconfig, PATH holds the pinned provider" (`src/network/tunnel.rs`). A kubectl URI names the cluster endpoint and the context (`README.md`, providers table). Lade also checks that the user's kubeconfig ties the context to that endpoint, so a context name that now points at another cluster fails instead of forwarding to the wrong one (`crates/lade-sdk/src/providers/network/kubeconfig.rs`). No doc states that check, so it is the code's and not a law here.

"A hub tunnel keeps the env of the command that started it, like a cached secret: a restart runs with those credentials, even after another shell reuses it" (`docs/tunnels.md`). The release note names what that env carries: "started with the environment of the first (`KUBECONFIG`, tsh identity, ssh agent)" (CHANGELOG 0.20.0, Every tunnel runs in the hub). Taking the env of the latest command instead would let a restart switch credentials under a command already running.

**§ <a id="n6qm5smv5">n6qm5smv5</a> A tunnel provider is prepared in the Lade process of the command that starts the tunnel, where the user's terminal and env are, and runs in the hub in a session of its own, with that command's env.**

- (a) A login the provider needs, `tsh kube login`, runs in that Lade process before the hub starts the provider, and stops when the command is cancelled.

- (b) **Retired.**

- (c) The provider, and every restart of it, runs with the env of the command that started the tunnel, even after other commands reuse it.

- (d) The provider runs in its own session with its input closed.

## Where a provider listens

Lade binds the user's port, and the provider listens somewhere else that Lade relays to, "on a port it picks and prints (`kubectl :remote`, `tsh proxy app` without `--port`) or on a Unix socket (`ssh`)" (`docs/tunnels.md`). The reason is the race of the port claim, one level in: "`tsh proxy app` picks its own port and prints it, like `kubectl port-forward`, so no other process can take the port between Lade choosing it and tsh binding it" (CHANGELOG 0.20.0, `tsh://` app tunnels). A port Lade chose and handed to the provider could be taken in between, and the relay would carry the user's traffic to whoever took it. Tests: `supervisor_waits_for_the_printed_port`, `printed_addr_reads_kubectl_and_tsh_ready_lines`, `relay_pipes_to_a_unix_socket_upstream`.

"`kubefwd` is the exception: it binds its own addresses, so Lade checks the port is free, then lets go before kubefwd starts. A restart can lose it" (`docs/tunnels.md`; `README.md`, Tunnel). What happens when it does is [bkmpmswkm.e](providers.md#bkmpmswkm).

**§ <a id="jfg5ews57">jfg5ews57</a> A relayed provider listens where no other process can take its place, on a loopback port it picks and prints or on a Unix socket Lade names.**

- (a) kubectl and tsh pick their own loopback port and print it, and Lade relays to the address they print.

- (b) ssh forwards to a Unix socket at a path Lade creates for that tunnel.

- (c) kubefwd binds the user's port itself, so Lade holds that port until right before kubefwd first starts, and kubefwd then serves it directly.

## Ready and failed

A tunnel is ready when the command can use it, which is when its own provider listens, not when something answers on the port: that was the 0.20.0 clash. kubefwd listens on the user's port itself, so Lade "counts the tunnel ready only when kubefwd itself listens on it" (CHANGELOG 0.20.0, Tunnel port clash; test `direct_does_not_ready_when_a_stranger_binds`).

A failed start says why. "A provider that exits at once, such as `tsh` when logged out, reported an empty 'timeout waiting for the tunnel to listen'. It now fails with the provider's own output" (CHANGELOG 0.20.0, Tunnel start errors). The command waits "past the supervisor's 60 s startup budget, so its error wins" (`src/network/binding.rs`). A provider that fails once is retried while the budget allows, and a retry that could not listen in time is not started, since "its empty log would replace this child's error" (`src/network/process/policy.rs`; tests `supervisor_retries_a_startup_failure`, `startup_fails_once_no_retry_can_start_before_the_deadline`). "Start fails: Report the error to every asker until their leases go" (`docs/tunnels.md`): two commands waiting on one start both get its error. The 60 s is documented ("A pre-exec tunnel gets 60 s to start, was 20 s", CHANGELOG 0.20.0) without a reason for that number, so the law names the budget and not its value.

**§ <a id="v29epeav2">v29epeav2</a> A tunnel is ready only once its own provider listens, and a start that fails gives every command that asked for it the provider's own error.**

- (a) A relayed tunnel is ready once the address its provider printed, or its socket, accepts a connection.

- (b) A kubefwd tunnel is ready once a process of its provider's own process group listens on the port, and another process of the user listening there fails the start.

- (c) A provider that exits or does not listen within the startup budget fails the start with its own output, once no retry could still listen within that budget.

- (d) The asking command waits longer than the startup budget, so it reports the provider's error rather than a timeout of its own.

- (e) Every command that asked for a start that failed gets that start's error, as long as it holds its lease.

## Restart

"Every tunnel runs in the hub, which restarts the provider if it exits" (`README.md`, Tunnel). A kubectl port-forward drops when its pod restarts, an ssh link when the laptop sleeps, and a dead provider behind a held port is a tunnel that accepts connections and goes nowhere for as long as its ttl. 0.17.1 started to "supervise command-scoped forwards and restart a provider that exits during the command, instead of leaving the local port dead" (CHANGELOG 0.17.1, Network tunnel restart). 0.20.0 fixed two ways it stopped restarting: "a provider that exited more than 60 s after the tunnel started was never restarted. Each restart now gets a full 60 s, and a helper the provider left behind (a `ProxyCommand`) no longer blocks it" (CHANGELOG 0.20.0, Tunnel restarts). Lade runs ssh "with `ServerAliveInterval=15`, so a dead link exits and restarts instead of hanging" (CHANGELOG 0.20.0, `ssh://` tunnels): an ssh that hangs never exits, so it is never restarted.

The backoff, "250 ms, doubling, 5 s cap" (`docs/tunnels.md`), keeps a provider that dies at once from spinning, and goes back to its floor once a restart is ready. The numbers have no stated reason and stay out of the law. The supervisor's policy is a pure function checked over every history to depth 14 (`every_supervisor_history_keeps_the_invariants`, `a_restart_an_hour_in_gets_the_full_startup_budget`, `a_provider_restarts_past_a_helper_holding_its_logs`).

kubefwd binds the user's port itself, so between two runs the port is free, and "a restart can lose it" (`docs/tunnels.md`). A tunnel whose port another program took must not stay listed as ready: the next command to that target would reuse it and reach that program, the very failure the port claim exists to prevent. Lade fails such a tunnel, so no command reuses it, whether the stranger listens beside kubefwd on another address or kubefwd exits at once because the port is taken.

Lade finds who listens on a port through `/proc/<pid>/fd` on Linux and `lsof` on macOS, both run as the user (`src/network/process/direct.rs`), so it sees the user's own processes only. A listener of another user is invisible to it: a kubefwd that lost its port to one reads as a plain exit and is restarted while the tunnel stays ready, and one that binds another address of the port during a first start does not fail it. No doc promises other users' listeners, so (b) and (e) name the user's own, and this is a stated limit.

**§ <a id="bkmpmswkm">bkmpmswkm</a> The hub restarts a tunnel's provider each time it exits, for as long as the tunnel lives.**

- (a) A restart waits a delay that grows from a floor to a cap, and the delay returns to the floor once a restart is ready.

- (b) Every restart gets the full startup budget to listen, however long the tunnel has run.

- (c) Before a restart, Lade kills whatever the exited provider left in its process group.

- (d) An ssh provider probes its link, so a dead link makes it exit and restart instead of hang.

- (e) A kubefwd restart that finds another process of the user listening on its port fails the tunnel.

## No orphan provider

"Every provider runs under `lade __tunnel-guard` in its own session. The guard's stdin is a pipe whose other end only its Lade holds. The kernel closes it however that Lade ends, `SIGKILL` included, and the guard then kills its process group. No provider outlives its Lade" (`docs/tunnels.md`). A provider left running holds a path into a private network, with the user's credentials, that no command accounts for. 0.20.0: "A provider whose helpers ignore `SIGTERM` is killed with its whole process group. A `kubectl`, `ssh`, or `tsh` whose Lade or hub was killed (`SIGKILL`, crash) is stopped by a guard process instead of running on" (CHANGELOG 0.20.0, Tunnel cleanup). From the `docs/tunnels.md` table: "Hub exits, `SIGTERM`, or `SIGINT`: Stop every tunnel, cancel starts, then exit". When the hub exits is the hub note's. That its tunnels go with it is this one's.

ssh can share one connection between sessions: a master from `~/.ssh/config` "would own the forward, so killing this ssh would not close it" (`crates/lade-sdk/src/providers/network/command.rs`). So "Lade runs its own ssh connection (`ControlPath=none`)" (`README.md`, Tunnel).

Tests: `test_tunnel_guard_stops_the_provider_once_its_lade_is_gone`, `test_killed_lade_leaves_no_provider_behind`, `test_tunnel_guard_passes_the_provider_exit_code`, `drop_stops_a_provider_group_that_ignores_sigterm`, `dropping_supervisor_terminates_the_provider_process_group`, `hub_exit_stops_a_starting_provider`, `releasing_a_starting_tunnel_stops_its_provider`, `stop_cancels_a_start_and_its_provider`.

**§ <a id="vdbmtzqtb">vdbmtzqtb</a> Every tunnel provider ends at the latest with the Lade process that runs it.**

- (a) Every provider runs under `lade __tunnel-guard`, whose input is a pipe only its Lade holds, and once that pipe closes the guard kills the provider's process group.

- (b) Stopping a tunnel kills the provider's whole process group, members that ignore `SIGTERM` included.

- (c) An ssh provider opens its own connection, with `ControlPath=none`.

- (d) Before the hub exits, it cancels every start and stops every tunnel.

## What stays out

The URI forms and query keys of each provider (`README.md`, providers table) are each provider's API, not a decision of Lade's. A jump host without a port takes it from `~/.ssh/config` (CHANGELOG 0.20.0, `ssh://` tunnels), which is ssh's own lookup. The poll intervals, the 16 tries for a free port, the 64 KiB log tail and the 5 s upstream connect timeout are values with no stated reason.
