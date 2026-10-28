# Tunnels

The hub that caches secrets ([cache.md](cache.md)) runs every tunnel,
from pre-exec, `lade --`, and harnesses alike, so one place knows every
Lade tunnel on a port. Without a hub there is no tunnel: the command
fails like any other acquisition. URIs and rule syntax are in the
README.

Lade binds the local port itself and relays each connection to the
provider, which listens on a port it picks and prints (`kubectl
:remote`, `tsh proxy app` without `--port`) or on a Unix socket
(`ssh`). Holding the port is the claim. A second tunnel
on that port gets `AddrInUse` at once, from this hub, another hub, or
any other process. macOS only refuses the identical address, so a
fixed port also counts as held when `0.0.0.0`, `::`, `127.0.0.1`, or
`::1` is taken on it. The kernel frees the port when its holder dies,
so there is no lock file to clean up. A provider restart keeps the
port: nothing else can take it in between. `kubefwd` is the
exception: it binds its own addresses, so Lade checks the port is
free, then lets go before kubefwd starts. A restart can lose it.

| Event | Hub |
|---|---|
| Ask, same port, same target | Reuse. Adds the lease. The ttl becomes the longer one |
| Ask, same port, other target, holder in use or starting | Refuse, naming the holder's key and URI. The client asks again every 500 ms for up to 10 min |
| Ask, same port, other target, holder idle (ttl only) | Stop the idle tunnel, then start the new one on its port |
| Ask, port held by another process | Refuse. The client gives up at once |
| Start fails | Report the error to every asker until their leases go |
| Provider exits | Restart with backoff (250 ms, doubling, 5 s cap). Each restart gets a full 60 s to listen |
| `lade unset` | Release the ticket's lease |
| Leasing process gone (pid and start time) | Release its lease, checked every second |
| Last lease gone, no ttl | Stop now, also mid-start |
| Last lease gone, ttl | Stop once the ttl runs out idle |
| `lade ttl forget [KEY]` | Stop now |
| Hub exits, `SIGTERM`, or `SIGINT` | Stop every tunnel, cancel starts, then exit |
| Hub killed (`SIGKILL`, crash) | Each `lade __tunnel-guard` sees its stdin close and kills its provider |

`lade set` opens pre-exec tunnels under its own lease, so a Ctrl-C
while they connect leaves nothing behind. Right before it prints, the
terminal session leader's lease takes over, since `lade set` exits
before the command runs. A `lade --` lease belongs to that process.

Each Lade binary has its own hub, so a tunnel of another Lade binary
on that port looks like any other program: refused, not waited for.

When a tunnel or a secret fails, for any reason, the command is
cancelled. A human at a terminal first gets 5 s to press Enter and run
it anyway, without Lade. No terminal (a harness, CI), no answer, or
Ctrl-C cancels it. `lade --` exits. A shell hook cannot cancel the
command, so `lade set` prints code that sets `PATH` to an empty
directory and erases `LADE`, so the hook skips `lade unset`, which has
nothing to undo. The command, its aliases, and the functions that wrap
it find nothing to run, and the shell's command-not-found handler
prints `Lade cancelled <command>.` bash before 4.0, such as macOS's
`/bin/bash`, has no such handler and prints its own `command not
found`. `PATH` and the user's handler come back before the next hook
in zsh and bash, and before the prompt in fish. In fish, the code
first lets psub remove the file it was sourced from, since psub does
that with `command rm`. A command typed with its full path still runs.

Ctrl-C while Lade connects cancels the command the same way. fish
aborts its pre-exec hook on SIGINT and runs the command anyway, so in
fish, and during the 5 s question in every shell, Lade reads the
terminal without signals and gets Ctrl-C as a key. Keys typed then
are not passed on to the shell.

Stopping a tunnel waits for its provider, so it runs off the hub's
request loop. A client whose request times out pings the hub for a
second before calling it stale: replacing a busy hub would stop its
tunnels.

Every provider, in the hub or in `lade --`, runs under
`lade __tunnel-guard` in its own session. The guard's stdin is a pipe
whose other end only its Lade holds. The kernel closes it however
that Lade ends, `SIGKILL` included, and the guard then kills its
process group. No provider outlives its Lade, and the `pid=` in the
progress line is the guard's.

A tunnel provider has no terminal, so it cannot ask for a password or
a code: authenticate it beforehand (an ssh key or agent, `tsh login`,
a kubeconfig that needs no prompt). A secret provider can ask: see
[README.md](../README.md#secret).

A hub tunnel keeps the env of the command that started it, like a
cached secret: a restart runs with those credentials, even after
another shell reuses it. Replacing the Lade binary stops the old hub
within a minute, and its tunnels with it, also under a running
command.

The lifecycle is a pure state machine (`src/hub/tunnels/machine.rs`).
Its tests walk every event sequence up to depth 8 from an empty hub,
plus long random walks, and check after each event: one tunnel per
port, no reuse across targets, no leased tunnel stopped, nothing idle
past its ttl, effects matching the machine, and the hub draining once
leases die. The supervisor's restart policy
(`src/network/process/policy.rs`) is checked the same way.
