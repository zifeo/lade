# Hub security

The hub is a second process of the same Lade binary, `lade __hub`, that outlives one command. It holds what several commands share: resolved secrets for their ttl, the tunnels it runs for every door ("The hub that caches secrets runs every tunnel, from pre-exec, `lade --`, and harnesses alike, so one place knows every Lade tunnel on a port", `docs/tunnels.md`), a copy of each ticket, the `lade ttl set` windows, and what it last learned about the latest Lade release (`docs/cache.md`, What the hub holds and Who talks to it). The cache policy, what is cached, for how long and under which key, is [cache](../cache/scope.md). The tunnels themselves are `docs/tunnels.md`'s. These notes decide the process: this one who may talk to it and what keeps its memory private, [lifecycle](lifecycle.md) how a client finds the right hub and when it ends, [wire](wire.md) what crosses its socket, and [notices](notices.md) the release check and notices it hands out.

The problem is that the hub is a secret server running under the user's account. "Values cross the socket and sit in the hub's RAM in plaintext" (`docs/cache.md`, What the hub holds), and every other program the user runs, an agent's tools included, is a process of the same user next to it. The promise is that the hub serves the processes of its user and protects the values it holds against other users, [keyfb5k7w](../promises.md#keyfb5k7w): the hub's scope is the user's account (owner answer, 2026-11-19). The kernel's word on the uid that connected, and a socket only that uid may open, hold the line between users. Within the account, the kernel's word on which binary connected and a memory no debugger can attach to keep values to the hub's own binary for every program of the user that asks it, an agent's tools included. A process that works against its own user, by listening on the hub's socket before a hub runs, by loading its code into `lade`, or by execing `lade` over a socket it keeps, acts with the user's own rights, and the user's account is the boundary the hub keeps. Beside security, the hub must never slow or break a command: a cache that costs more than the provider, or a hub left behind by an upgrade serving the old binary's tunnels, is worse than no hub. And it must cost nothing to a command that does not need it.

## The memory guard

A peer check is worth nothing if another process of the user can read the hub's memory with a debugger. "The hub caches a secret only where no other process of the user can read its memory. Elsewhere it caches none: every command hydrates from its providers, and the hub still holds tickets and tunnels. No step needs `sudo` or an Apple account" (`docs/cache.md`, Memory guard; CHANGELOG 0.20.0, Who may read the cache). On Linux any process may make itself not dumpable, which closes `ptrace` and `/proc/<pid>/mem` to the user's other processes. On macOS that protection comes from the hardened runtime flag in the binary's signature: release binaries and `scripts/install-local.sh` sign ad hoc, and a plain `cargo install` caches once the user signs it with `codesign --force --sign - --options runtime "$(command -v lade)"` (`README.md`, Install; CHANGELOG 0.20.0, Breaking). A macOS debug build, the one tests and `cargo run` produce, caches with no guard: cargo does not sign it, and a developer's own test run is the use (`docs/cache.md`, Memory guard table). A hub also sets no core file, since a core dump writes its RAM to disk. Root reads any process on either system; that is outside what Lade can promise.

The guard is a condition on the cache, not a best effort: a hub that could not make itself not dumpable must not go on caching as if it had.

**§ <a id="kgejwr97c">kgejwr97c</a> The hub caches a secret only in memory guarded against the user's other processes, a macOS debug build aside.**

- (a) On Linux the hub makes itself not dumpable at start and then caches secrets, and a hub that cannot do so runs with the cache off.

- (b) On macOS the cache is on for a binary signed with hardened runtime and for a debug build, and off for any other build.

- (c) With the cache off, every command resolves its secrets from their providers, and the hub still serves tickets and tunnels.

- (d) The hub sets its core file size limit to zero at start, on every system, and a hub that cannot do so runs with the cache off.

- (e) On macOS, the release binaries and `scripts/install-local.sh` sign `lade` ad hoc with hardened runtime.

The release half of (e) is decided in `.github/workflows/release.yml`, which signs with `--options runtime` and checks the flag. Silcrow does not parse a workflow, so no marker pins that copy: only `scripts/install-local.sh` carries one, and a change to the release signing goes unseen by `silcrow check`.

## Who may talk to the hub

The client's word is worth nothing: anything it sends, a version, a name, a token, another program can send too. So the hub asks the kernel about the process at the other end of the socket: "The hub serves a connection only when the kernel vouches for the client" (`docs/cache.md`, Who may talk to the hub). On macOS the kernel names a process's code by its code directory hash, and the audit token from `LOCAL_PEERTOKEN` names that exact process, so "a reused pid or an `exec` since the connect fails" (`src/hub/guard.rs`). Without a signature, an Intel `cargo build`, and on Linux, the hub compares the client's executable file, by device and inode, with its own. The hub holds its own file open from the start, so the inode it compares with cannot be handed to another file after a reinstall (`src/hub/peer.rs`, `ImageSnap`). A client the hub does not vouch for gets `Err` 4, "denied (wrong peer or version)" (`docs/cache.md`, Wire, Failures), before the hub reads a byte of its request.

**§ <a id="e1s6kxps0">e1s6kxps0</a> The hub takes a client's identity from the kernel, for the process at the other end of the socket, and serves only a connection the kernel vouches for.**

- (a) The client's uid, as the kernel reports it for the socket, equals the hub's.

- (b) On macOS, when the hub's binary is signed, the client's audit token names the connected process, its code is valid, and its cdhash equals the hub's.

- (c) Otherwise, the client's executable is the hub's own file by device and inode, the hub holding that file open from its start.

- (d) A client that fails the check gets `Err` 4 before the hub reads its request.

Only the macOS audit token is proof against an `exec`. On Linux, and on macOS for an unsigned build, the check names the pid the kernel recorded at connect and reads that process's executable at accept, through the path `proc_pidpath` returns on macOS. A process of the user that connects, forks a child that keeps the socket, and then execs `lade` passes, and so does `lade` run with a loader the user injected (`LD_PRELOAD`). A client takes whatever listens on `hub-<token>.sock` for its hub, so a process of the user that binds the socket while no hub runs receives the values a client puts. Each acts with the user's own rights, inside the account the hub serves ([keyfb5k7w](../promises.md#keyfb5k7w)): the check sorts the user's programs that ask, and the uid of (a) is the line between users.
