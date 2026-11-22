# Remote resolution

Humans and harnesses run commands on remote machines, over `ssh` or Teleport's `tsh ssh`, and Orca's SSH targets are `~/.ssh/config` aliases of the first kind. Those commands need the secrets and tunnels their `lade.yaml` rules name, often both in one rule, a database password beside the tunnel to that database. The user wants the provider sessions and credentials, the password manager app, a Vault token, cloud profiles, kube contexts, Teleport certificates and ssh keys, on the laptop only. Often the laptop also orchestrates the remote work, a harness on the laptop driving commands on the remote.

The model is a resolution delegated to the laptop, nothing more. `lade connect <path> -- <transport>` reads the remote's `lade.yaml` files at that path, resolves them on the laptop as a local Lade would, with the laptop's providers, and caches what it resolved. The laptop hub keeps one link to the remote, and the remote's commands get their values and tunnels over it. The design is `docs/remote.md`.

There is no approval step of Lade's own. The files are shown when they are resolved, and the providers ask what they ask locally: an unlock, a Touch ID, a `tsh login`. A disclaimer stays where the command runs, on the remote, as it does locally. Locally, a harness that edits `lade.yaml` gets the new values on its next command, and the remote keeps that parity: what binds is the file the laptop resolved, named by the hash Lade already computes over the walk ([je3g2tk0d](cache/scope.md#je3g2tk0d)) with the remote's saved Lade user, whose per-user entries the laptop resolves, and everything is in the laptop's diary.

Values handed to the remote are readable by the remote user's own processes and by root while linked. That is the scope of this note.

## What resolves where

What goes up is what needs a credential: every secret provider, and the tunnels to a cluster, a Teleport proxy or an ssh jump. What stays is what only the remote holds or must run itself: a `file://` or a `sops://` reads a file of the remote, a shell source is code of the remote and runs there, raw values and templates are composed from the values once they arrive, packages are binaries for the remote, and `kubefwd` binds its own addresses on the machine where the command runs. A hybrid rule splits along that line on its own.

A remote that falls back to a local session when the laptop is away would resolve the same rule from another account, or from a token left in its environment, with no sign, and telling a provider that is logged out from one that never was is guesswork. Attached mode is a marker `lade attach` writes, so the switch is explicit and survives the link going down.

**§ <a id="a36hqp4ep">a36hqp4ep</a> While a remote machine is attached to a laptop, Lade there resolves every secret provider ref, and starts every tunnel but `kubefwd`, through the laptop.**

- (a) Raw values, templates, `file://`, `sops://`, shell sources, packages and `kubefwd` resolve and run on the remote, as on any machine.

- (b) A command that needs the link while it is down fails with exit 4 and prints the `lade connect` line to run on the laptop.

- (c) A request through the link waits longer than the laptop's own provider bound before it fails with exit 4, so a prompt on the laptop does not fail it.

- (d) `lade status --json` reports, on both sides, whether the machine is attached and whether its link is up.

A detach on the remote while the link is down would otherwise be undone by the laptop's next retry, and a detach on the laptop while it is down would leave the remote failing with exit 4.

**§ <a id="q4qz9zrdn">q4qz9zrdn</a> `lade detach`, on either side, ends attached mode on the remote, also while the link is down.**

- (a) A link the laptop hub restarts gives up once the remote was detached, until the next `lade connect`.

- (b) On the laptop, with the link down, `lade detach` runs `lade detach` on the remote over the transport.

## Connecting

`lade connect` is run once per path, as often as there are projects on the remote, and every run over the same transport shares the one link the hub holds. A harness that runs it must know whether the link is there when it returns, and must not be left with a process to babysit.

**§ <a id="ycvygtrew">ycvygtrew</a> `lade connect <path> -- <transport>` resolves the `lade.yaml` walk of the remote at that path on the laptop, with the laptop's own providers, and caches what it resolved.**

- (a) It shows every file of the walk, with the text from the remote escaped so it cannot move the terminal's cursor.

- (b) It exits 0 only once the walk is resolved and the link is ready, and otherwise exits with a named reason.

- (c) A later `lade connect` over the same transport adds its walk to the link already running.

The laptop never resolves a string the walk does not yield: a remote that names any ref in a request would reach any secret the laptop's credentials reach. The walk hash names which files the request comes from, and the laptop answers from its own resolution of them. A ref templated on a value of the remote, a `file://` or another body's binding, can only be rendered there: the laptop resolves the ref the remote rendered when a template of the walk yields it, so the part the remote fills is the one the walk gives it.

**§ <a id="d0jxpxadk">d0jxpxadk</a> The laptop answers a remote's request only with what the walk the request names yields: its own resolution of it, or a ref the remote rendered from one of its templates.**

- (a) A request whose walk no `lade connect` resolved fails with exit 6 and prints the `lade connect` line for its directory.

- (b) A ref or tunnel URI that the laptop's resolution of that walk does not yield, and no template of it yields, fails with exit 5 and names it.

- (c) A value past its ttl is resolved again on the laptop, and a failure there fails the request with exit 4, the provider's error and the `lade connect` line.

## Tunnels through the laptop

The remote keeps the tunnel's port, the lease of each command on it and its restarts, as today: only where the provider runs changes. Its connections cross the link as streams to a tunnel the laptop hub runs with the laptop's credentials, under the laptop's own tunnel rules. The bytes are the service's own protocol, encrypted on the way by the transport.

**§ <a id="aswq5qe7f">aswq5qe7f</a> An attached remote binds each tunnel's port itself and carries every connection to it over the link to a tunnel the laptop hub runs for it.**

- (a) The laptop keeps that tunnel while a remote lease on it lives, and the link holds that lease for the remote.

- (b) A laptop tunnel that fails to start fails the remote command with the laptop provider's own error.

- (c) The tunnel's credentials and provider process stay on the laptop, and only the connection's bytes cross the link.

- (d) The laptop checks each command's tunnel against the walks it resolved, also when the remote reuses a running tunnel, and a tunnel through the link is never reused for one with the remote's own credentials, or the reverse.

- (e) Each connection moves at its own pace: a slow socket holds back no other connection and no request of the link.

## The link

`ssh` and `tsh ssh` both authenticate the host before the pipe carries anything. Other pipes, `kubectl exec` or `docker exec`, need a fingerprint check first, so the accepted list is closed until they get one. Lade runs the caller's command line as written, an alias of `~/.ssh/config` included, and appends its own remote command, so only what would break that pipe is refused.

**§ <a id="bhj0hmdfv">bhj0hmdfv</a> `lade connect` runs its transport as the caller wrote it after `--`, `ssh …` or `tsh ssh …`, and appends its own remote command.**

- (a) Lade refuses what would break the link's pipe: a tty, a session with no remote command or in the background, and a remote command of the caller's.

- (b) Lade refuses any other transport, naming the accepted ones.

Laptop and remote often run different Lade versions, since each machine upgrades on its own. The link has a protocol version of its own, apart from the hub's, so a mismatch is named instead of read as garbage.

**§ <a id="adpzyyt56">adpzyyt56</a> Both ends of a link exchange their protocol versions before any request, and a mismatch fails the link naming both.**

The hub runs the link as it runs tunnel processes, and never waits on it: its connections stay in-memory table work.

**§ <a id="dzex6kwmq">dzex6kwmq</a> The laptop hub keeps one link per transport until it is detached or the hub exits, and never handles the values it carries.**

- (a) After a transport drop, the hub restarts the link without a terminal, and stops retrying on a failure only a human can clear.

- (b) A link the hub restarts gives way to another live link of the same laptop to the same remote account, so two transports to one machine do not end each other in turn.

SSH already encrypts the pipe. The per-request key keeps plaintext off everything between the laptop resolver and the requesting client, `attach`, the transport's own processes and any relay or debug log, whatever the transport becomes. It does not protect the endpoint.

**§ <a id="rp9r1x8pj">rp9r1x8pj</a> The laptop encrypts the values of each request to a key the requesting remote client generated for that request.**

**§ <a id="jzxqp2f1m">jzxqp2f1m</a> A remote account is attached to one laptop at a time: a `lade connect` from the same laptop over another transport replaces its link, and one from another laptop is refused.**

**§ <a id="g1qjsg8hr">g1qjsg8hr</a> The remote serves the link only to processes of its own user.**

## The record

The laptop is the only side the user trusts, so the record lives there and does not depend on a `log` flag in files the remote controls. What the remote reports about the command is kept as reported.

**§ <a id="b4tp4hvxk">b4tp4hvxk</a> The laptop records each `lade connect`, each request it serves or fails, each link loss and each detach, whatever the rules' `log` says.**

- (a) A row keeps the host the laptop dialed, the walk hash and the refs as the laptop's own, and the command, cwd, audience and matches as the remote reported them.

- (b) A connect row records whether a human or a harness ran it, the path, its files, and every ref and tunnel it resolved.

- (c) A value the laptop served is replaced wherever the remote's report repeats it.
