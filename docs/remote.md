# Remote resolution

Status: implemented, laws in `docs/decisions/remote.md`.

## Goal

Humans and harnesses run commands on remote machines (plain SSH, Teleport `tsh ssh`, Orca SSH targets), and those commands need the secrets and tunnels of their `lade.yaml` rules. The provider sessions and credentials stay on the laptop: no password manager, Vault token, cloud profile, kube context, Teleport certificate or ssh key on the remote. Often the laptop also orchestrates the remote work.

Remote resolution is a resolution delegated to the laptop, nothing more. `lade connect <path> -- <transport>` reads the raw `lade.yaml` walk of that path on the remote, shows it, and resolves it on the laptop with the normal Lade engine and the laptop's providers. What a provider asks locally, an unlock, a Touch ID, a `tsh login`, it asks there, interactively. The results are cached on the laptop and sent encrypted to the remote. Everything is in the laptop's diary.

## Non-goals

- Protecting values from the remote user's own uid or root while linked.
- Unauthenticated transports (`kubectl exec`, `docker exec`, Orca paired runtimes without SSH). The link is a stdio pipe, so they can come later, with a fingerprint check.
- Running code the remote chose on the laptop: shell sources run on the remote.
- Windows remotes. Lade is Unix-socket only.

## Shape

| Piece | Where | What it is |
|---|---|---|
| `lade connect <path>` | Laptop | Fetches the walk of `<path>` with `lade __remote-files`, shows it, resolves it on the laptop, caches it, and adds it to the hub's link over that transport |
| `lade __remote` | Laptop, run by the hub | `<transport> lade attach`. Holds the walks `lade connect` resolved, answers each request from them, encrypts, writes the diary |
| `lade attach` | Remote, at the end of the transport pipe | Serves `<cache root>/up/sock`, relays requests and replies over stdio, writes the attached marker |
| `Upstream` provider | Remote, in `lade-sdk` | Replaces every secret provider in attached mode. Every surface (hooks, `inject`, `eval`, `age-plugin-lade`) follows with no per-surface code |
| `lade __linked-tunnel` | Remote, run by the remote hub as a tunnel provider | Listens on a Unix socket Lade names and carries each connection over the link to a tunnel the laptop hub runs |

Both hubs stay off the data path: a hub serves one connection at a time with a 500 ms cap, right for in-memory tables, wrong for a multi-second provider call.

## What resolves where

| Source | Attached remote |
|---|---|
| Secret providers: `op`, `infisical`, `doppler`, `vault`, `passbolt`, `bw`, `awssm`, `azurekv`, `gcpsm`, `age` | Laptop, through the `Upstream` provider |
| Tunnels: `kubectl`, `tsh`, `ssh` | The laptop runs the tunnel. The remote binds the port and carries each connection over the link |
| `kubefwd` | Remote: it binds its own addresses on the machine where the command runs |
| `file://`, `sops://` | Remote: they read a file of the remote |
| Shell sources (`sh://`, `bash://`, `zsh://`, `fish://`) | Remote: code of the remote |
| Raw values and templates | Remote, composed from the values once they arrive. A ref templated on a value of the remote, a `file://` or another body's binding for instance, is rendered by the remote: the laptop resolves the ref it receives when the template yields it |
| Packages (`mise://`) | Remote: binaries for the remote |

A hybrid rule needs nothing special:

```yaml
"^psql":
  DB_PASSWORD: vault://vault.acme.dev/kv/app/db/password   # laptop
  DB_PORT: kubectl://k8s.acme.dev:6443/prod/db/service/postgres/5432 # laptop runs the tunnel, remote binds the port
  DATABASE_URL: postgres://app@127.0.0.1:${DB_PORT}/app    # remote, filled from the two above
```

## Connect

```
lade connect src/app -- ssh -J bastion me@build-01
lade connect src/worker -- ssh -J bastion me@build-01
lade connect ~/infra -- tsh ssh me@build-02
```

1. The transport runs once with `lade __remote-files <path>`, in the caller's terminal, so host key, passphrase, 2FA and agent prompts reach the user. With `ssh` it opens a `ControlMaster` with a short `ControlPersist` that `__remote` rides. The answer holds the remote hostname, saved user, Lade and protocol versions, the directory, and the paths and bytes of every `lade.yaml` on its walk, `$HOME`'s included.
2. The laptop shows those files, escaped so remote text cannot move the cursor.
3. It asks the hub to start the link over that transport, if none runs, while the master of the fetch is still there to ride.
4. It resolves every rule body of the walk as a local Lade would, with the remote's saved user for per-user entries, and only what goes to the laptop: secret provider refs, private ones included, and the raw values their templates use. A body's config resolves there with it, a service account token included, which stays on the laptop. Each body is cached in the laptop hub under the walk hash, with the body's own ttl. A body that fails to resolve fails `lade connect`, with the provider's error. A provider ref whose template needs a value its body does not resolve on the laptop, one only the remote has or another body's, is listed as resolved on first use: the laptop resolves it when the remote asks for it.
5. It adds the walk to the link and waits until the link is ready and `__remote` has taken the walk up. Exit 0.

The path is relative to the remote home. Each directory with its own walk needs its own `lade connect`. Every `lade connect` over the same transport shares one link and one `lade attach`.

Everything after `--` is the transport, run as written: `~/.ssh/config` aliases and Orca SSH targets work as they are. Lade adds `-T -o RemoteCommand=none` for `ssh`, and its own remote command. It refuses only what would break the pipe: a tty (`-t`, `RequestTTY`), no remote command (`-N`, `SessionType`), a closed stdin (`-n`, `StdinNull`), the background (`-f`, `ForkAfterAuthentication`), a subsystem, stdio forward or control command in place of the session (`-s`, `-W`, `-O`), a printed config (`-G`), and a remote command of the caller's. Any transport but `ssh` and `tsh ssh` is refused, naming them.

## Serving

Every config load hashes the `lade.yaml` files on its walk. A request from the remote carries that hash combined with the remote's saved Lade user, the one the laptop resolved the per-user entries for. `__remote` answers from its own resolution of the walk with that hash, never from the refs the remote sends:

- A walk no `lade connect` resolved, an edited, checked out or new `lade.yaml` or a `lade user` change included, fails with exit 6 and the `lade connect` line for that directory.
- A ref the remote rendered from a template of the walk is resolved on the laptop with its body's config when that template yields it, the last such binding of the walk as a later rule overrides an earlier one locally, and cached apart.
- A ref or tunnel URI the laptop's resolution of that walk does not yield, and no template of it yields, fails with exit 5, naming it, with the `lade connect` line.
- A value past its ttl is resolved again on the laptop, once for concurrent requests. A failure there, a logged out provider for instance, fails with exit 4, the provider's error and the `lade connect` line.

The remote can lie about its walk hash. That gains nothing: the laptop only serves what the files it showed yield. A template yields whatever its references render: the part of a ref a `file://` of the remote fills is the remote's to choose, as the walk says.

Disclaimers, `when:` and the other per-command rules apply where the command runs, on the remote, as they do locally.

## On the remote

Nothing to install beyond `lade`. `lade attach` writes `up/attached.json` with the laptop's name and the transport as written, so messages name the command to run:

```
This machine is attached to me@laptop but the link is down (vault://…/password): lade attach is not running. Ask the user to run on the laptop: lade connect /home/me/src/app -- ssh -J bastion me@build-01. Or stop using the laptop here: lade detach
```

| Exit | Name | When |
|---|---|---|
| `4` | `UPSTREAM_ABSENT` | Link down, request deadline passed, or the laptop's provider failed |
| `5` | `UPSTREAM_REFUSED` | A ref or tunnel the laptop's resolution does not yield |
| `6` | `WALK_UNKNOWN` | A walk no `lade connect` resolved |

There is never a fallback to a local provider or a local tunnel credential while attached. When not attached, the remote behaves exactly as before.

| Command | Effect |
|---|---|
| `lade detach -- <transport>` (laptop) | The hub stops `__remote`, and `attach` leaves attached mode. With the link down, the laptop runs `lade detach` over the transport instead |
| `lade detach` (remote) | Removes the marker, leaves `up/detached`, and stops `attach`. A link the laptop hub retries finds `up/detached` and gives up until the next `lade connect` |
| `lade status --json` | `remote.upstream` (`none`, `attached`, `broken`) on the remote, `remote.links` with each link's phase and walks on the laptop |

## Link lifecycle

The hub runs `__remote` as it runs a tunnel process and keeps the walks of each `lade connect` for it. It writes them on the lifeline of every `__remote` it spawns, as `w <walk> <json>` lines, and reads `walk <hex>` acks back on its stdout. A live link keeps the hub up.

1. `__remote` runs `<transport> -o BatchMode=yes -o ServerAliveInterval=15 -o ServerAliveCountMax=3 [-o ControlPath=…] lade attach`, with the `lade connect` caller's env.
2. Handshake: the laptop writes nothing until the remote's magic prelude, so shell rc output is skipped up to a cap. Then protocol and Lade versions both ways. A mismatch fails, naming both.
3. `attach` takes an exclusive `flock` on `up/lock`, binds `up/sock` (0600, dir 0700), and writes the marker atomically.

| Event | Result |
|---|---|
| Transport drops, laptop sleeps | Heartbeat every 15 s, `attach` exits after 45 s without one. The marker stays: exit 4. The hub restarts `__remote` with backoff, without a terminal, and hands it every walk again |
| Failure only a human clears (auth, version mismatch, another laptop attached) | `__remote` exits with a dedicated code and the hub stops retrying |
| New `lade connect` from the same laptop over another transport | Its link replaces the old one on the remote |
| A retried link while another `attach` holds the lock | It waits out the old one's heartbeat and never ends it. Still held: it gives up, so two transports to one remote do not end each other in turn |
| `lade connect` from another laptop while attached | Refused, naming the current laptop |
| Laptop hub exits | `__remote` dies with it. The remote gets exit 4 until the next `lade connect` |
| `lade connect` over a transport whose link gave up | A new link starts with the walks the old one held and the new one |

## Request flow

1. A remote command misses its hub cache for an upstream ref. The `Upstream` provider generates an X25519 key for the request and sends `{walk hash, refs, recipient, argv, cwd, audience}` to `attach`.
2. `attach` checks the peer uid and relays it to `__remote`, id-multiplexed.
3. `__remote` finds the walk and the body of each ref, or the template that yields it, takes the values from the laptop hub's cache or resolves them again, and encrypts them to the recipient with age.
4. The remote decrypts, injects, and caches as before.

The `Upstream` provider waits 35 s, above the laptop's own provider bound, so a prompt on the laptop does not fail the request.

## Tunnel flow

The remote hub keeps what it does for a tunnel: the port its key decides, each command's lease, readiness, restarts. Only the provider changes: `lade __linked-tunnel <uri>` listens on a Unix socket, and the relay pipes each accepted connection to it.

- `__linked-tunnel` asks `__remote` for the tunnel with the command's walk hash. `__remote` checks the URI against that walk, then asks its own hub for the tunnel as any command does, with a lease it holds while `__linked-tunnel` lives. The laptop's tunnel rules apply unchanged.
- Each command checks its tunnel against the laptop, also when the remote hub reuses a running one, and a linked tunnel is never reused for one with the remote's own credentials, or the reverse.
- A laptop tunnel that fails to start fails the remote command with the provider's error. The laptop hub gives up first (70 s), then `attach` (90 s), the remote supervisor's linked budget (100 s) and the remote command (110 s), so each error is shown before the next side gives up.
- Each stream has a credit window of 64 frames of 32 KiB, acked as written, so one slow socket holds back no other stream and no request.
- A link drop ends `__linked-tunnel`. The remote hub restarts it as any provider, which reopens the laptop tunnel once the link is back. Open connections are lost.
- Connection bytes are the service's own protocol, encrypted on the way by the transport.

## Diary

`Kind::Remote` rows, always written on the laptop whatever `log:` says. `host` is the one the laptop dialed, `reported_host` the name the remote gave itself. The walk hash and refs are the laptop's own; the command, cwd, audience and matches are as reported, scrubbed, with the values just served replaced.

| `event` | When |
|---|---|
| `connect` | A `lade connect`: human or harness, path, walk, files, every ref and tunnel it resolved |
| `served` | Refs served |
| `tunnel` | A laptop tunnel opened for the remote |
| `checked` | A remote command's tunnel checked, reuse included |
| `failed` | A provider or tunnel on the laptop failed, with its error |
| `refused` | Exit 5 or 6, with the reason |
| `broken` | Link lost, with the reason |
| `detach` | Link stopped, and from which side |

## Security model

| Threat | Covered by |
|---|---|
| Network observer | `ssh` or `tsh` |
| Anything between `__remote` and the requesting client (`ssh`, `tsh` and its proxy, `attach`, a relay, debug logs) | Per-request age encryption to the client's ephemeral key |
| Laptop hub | Never sees values in transit: it only runs `__remote` |
| A remote that asks for refs of its choosing | The laptop serves only what the walks it showed yield |
| A harness on the remote edits `lade.yaml` | Exit 6 until a `lade connect` shows and resolves the new walk |
| Tunnel credentials | Stay on the laptop: only connection bytes cross |
| Shell source run on the laptop | Never |
| Another uid on the remote | `up/sock` 0600 in a 0700 dir, peer uid check |
| Same uid or root on the remote | Not covered: it reads what the link serves while linked. `remote` rows on the laptop |
| Values already delivered after detach | Not covered: remote cache rows expire with their ttl, child envs stay |
| Remote impersonation | SSH host key or Teleport host certificate, and the user typed the destination |

## Open questions

1. Tunnel latency: every connection goes remote, laptop, service. Fine for a database session or an API call, maybe not for bulk transfers.
2. Silent reconnect with an approving ssh agent: an agent that asks for approval when a new process uses a key may pop a dialog on a reconnect by the hub, or fail and leave the link down (exit 4) until the next `lade connect`. `tsh ssh` avoids it while its certificate is valid.
3. A provider session that needs a terminal on the laptop: `__remote` runs under the hub, without one, so a value past its ttl that needs a prompt there fails with exit 4 and the `lade connect` line, which resolves it again in a terminal.

## Test plan

- `tests/remote.rs` runs both ends on one machine behind a fake `ssh`: connect and resolve, every secret provider, an edit that needs a new connect, refs templated on a remote file or another body, a disclaimer and `when` on the remote, a second path sharing the link, a second transport to the machine, tunnels through the link, a tunnel failure, detach on each side, link down, other transports refused.
- `tests/remote_sshd.bash`, run by `cargo test --test docker` (`tests/docker/remote.rs`), links a real `sshd` in a Debian container to this build: connect, the remote resolves through the laptop, a tunnel, a killed `lade attach` gives exit 4 and the hub restarts the link, an edit gives exit 6 then resolves after a new connect, detach, and the diary rows with no value.
- Never rebuild `target/debug/lade` while those tests run, and never kill hubs by name: a real hub may be running.
