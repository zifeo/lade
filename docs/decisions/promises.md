# Promises

Lade sells three things in one line (`README.md` title, `Cargo.toml` `description`): temporary access to secrets and private networks for one command, then gone; the same wrap for humans and harnesses; and a way to see which access was used. Everything else, the config walk, the providers, the hub, the tunnels, the pins, the diary, is machinery behind those three sentences.

The problem is that the machinery has four doors and three families. A command reaches Lade from the shell hook (pre-exec), from a harness's pre-tool hook, from `lade --` in a script, or as an MCP server behind `lade mcp`. Each door brings a secret, a tunnel, a pinned CLI, or several. A promise that holds on three doors and leaks on the fourth is the one a user discovers in production: a deploy that ran without its credentials and fell back to ambient ones, a tunnel left open on a laptop, a harness that got a different value than the human typing the same command. This note states the promises every door keeps.

## Reading order

In the order a command meets each topic: [configuration](config/discovery.md), [secrets](secrets/providers.md), [wrap](wrap/tickets.md), [harnesses](harness/audience.md), [secret cache](cache/scope.md), [hub](hub/security.md), [tunnels](tunnels/ports.md), [packages](packages/pins.md), [observability](observability/rows.md), [platforms](platforms.md), [development](development.md).

## One command, then gone

The README opens on it: "That access should exist for the process, then disappear" (`README.md`, title). The Patterns table repeats it per family: a private network is open "only while the command runs, then close it automatically", secrets as files are "temporary config files", and a package is put "first on PATH for this process" (`README.md`, Package). A tunnel kept alive by a ttl is the one sanctioned exception, and the README states it next to the rule: "A tunnel stops with its command. `ttl:` on the rule's `.`, or a `lade ttl set` window, keeps it that long after the last command that used it" (`README.md`, Tunnel). The `.file` output is the one sanctioned file on disk: "writes a temp JSON/YAML for the command, then deletes it" (`README.md`, Secret).

Pre-exec is the hard door. A shell hook cannot start the command itself, so Lade exports the values into the interactive shell right before the line runs and takes them back right after. The promise there is not that the shell never sees a value, it is that the shell holds it for that one command and returns to what it had before. For a remote MCP server there is no child process: the bindings become headers on the upstream requests that `lade mcp` bridges.

The command is not the only process that sees a value while Lade prepares it. A shell provider gets the bindings it is built from in its environment, [v3rjmav3z.c](config/bindings.md#v3rjmav3z), a provider gets its rule's 1Password service-account token, and Lade puts a pinned CLI first on its own `PATH`, so the providers and tunnel helpers it starts find it too. Those processes run for the command and end before or with it.

**§ <a id="jfxr4rmae">jfxr4rmae</a> Access a rule grants exists for one command and is gone when that command ends.**

- (a) On `lade --`, a pre-tool wrap and `lade mcp`, a secret, a tunnel's local port or a pinned CLI a rule adds reaches the command only through the process Lade starts for it, in its environment and, for a pinned `PATH`, the line it runs, or, for a remote MCP server, the headers of its upstream requests.

- (b) In pre-exec, the shell holds those values only for the command it is about to run, and gets its previous environment back when that command ends, a variable whose previous value was not UTF-8 text coming back unset.

- (c) A tunnel stops once no running command uses it, unless a ttl the rule or the directory sets keeps it for that long.

A rule's `.file` output is the third way a value reaches a command, and its lifetime, written for the command and deleted after it, is [fz8r6d2ka](secrets/output.md#fz8r6d2ka). The hub's secret cache keeps a resolved value in RAM between commands (`docs/cache.md`), which is not access: no process gets the value until the next matched command asks for it. How long a ttl may be, and what caps it, belongs to the cache and tunnel notes.

The restore of (b) has a copy per shell. Only the bash one, `scripts/on.bash`, carries a marker: Silcrow does not parse `scripts/on.zsh` and `scripts/on.fish`, so a change there fails no check. `eval "$(lade off)"` is itself a line the hook wraps, and it removes the after-hook that would restore the shell, so the `off` code runs that after-hook first, in each shell (`scripts/off.bash`; tests `{zsh,bash,fish}_lade_off_gives_the_shell_its_environment_back`). `lade unset` reads no `lade.yaml`, so a yaml broken while the command ran still restores (test `unset_restores_the_shell_when_lade_yaml_broke_during_the_command`).

The snapshot carries the `LADE_T` of the line that took it, and `lade unset` restores only the snapshot of its own line's ticket. A nested interactive shell inherits the outer command's `LADE_RESTORE`, `LADE_T` and `LADE_MISE_CONFIG`; when its own `lade set` fails before it prints, on a cache directory Lade cannot use for one, its `lade unset` finds a snapshot of another line, restores nothing from it, and leaves the outer command's mise config file in place, so the inner shell keeps the outer secrets and pinned `PATH` it held (test `a_nested_unset_restores_no_snapshot_of_the_outer_command`). Lade's own protocol variables, `LADE_T`, `LADE_RESTORE` and `LADE_MISE_CONFIG`, are cleared after every line, an inherited one included: they are Lade's bookkeeping, not the user's environment. An inner line spelled as the outer command reads the outer ticket as its own, as [fz8r6d2ka.e](secrets/output.md#fz8r6d2ka) states.

Three limits stay. The snapshot is JSON text, so a variable whose previous value is not UTF-8 is recorded as unset, and the restore unsets it; recording raw bytes would need a byte-exact export in each shell's quoting. A snapshot `lade unset` cannot decode holds no previous value: unset erases the variables the ticket names, so one that had a value before the command loses it, and a pin's variables stay, a pinned `PATH` included ([qmybbcmkx.b](wrap/preexec.md#qmybbcmkx)). A shell killed while its command runs never runs the after-hook, and keeps nothing to restore.

## Same wrap for humans and harnesses

"Humans use the shell hook (pre-exec). Harnesses use pre-tool hooks in the repo. Same `lade.yaml`, same resolve" (`README.md`, Getting started, again in Shell and harnesses). For MCP: "Same rule matching, disclaimers, and diary as `lade -- <cmd>`" (`README.md`, MCP). The 0.16.0 release put secret hydration and network acquisition on one shared path "so shell hooks and `lade inject` acquire the same command-scoped resources", and 0.20.0 made pre-exec `set` and a direct inject "read the same directory and line" as the pre-tool hook (`CHANGELOG.md`, 0.20.0, Pre-tool rewrite).

The reason is trust in one config. A team writes one `lade.yaml` and reviews it once. If the harness path resolved its own way, a rule could hold for the human and not for the agent running the same `tofu apply`, and nobody would read two configs to find out. The pre-tool hook therefore does not resolve anything: it rewrites the line into the same wrap a script would call. The one place the two may differ on purpose is a rule's `when: human` or `when: agent`, which the yaml states in the open; who runs the command picks rules through `when` and nothing else, [v03g22hz0](config/matching.md#v03g22hz0).

**§ <a id="g6sqmp8tb">g6sqmp8tb</a> Humans and harnesses get the same access for the same command from the same `lade.yaml`, whichever way the command reached Lade.**

- (a) A matched command gets the same access whether the wrap reads its plan from a pre-tool ticket or plans it from the rules.

- (b) A pre-tool hook hands a command with access to prepare to the `lade --` wrap, which resolves it.

- (c) `lade mcp` plans and acquires a server's access as `lade --` does, the package pins of its matched rules included.

- (d) A shell line, typed or sent by a harness, is matched against the `lade.yaml` walk of the directory it runs in, after its leading `cd <dir> &&` segments.

## Fail closed

"When a tunnel or a secret fails, the command is cancelled: it would run without what it needs, or reach the wrong listener" (`README.md`, Tunnel). The 0.20.0 release made this the rule on every path, pre-exec included, where the command used to run anyway after 5 s: "A failed secret, tunnel, pin, ticket, or env conflict cancels the command, in pre-exec too. Only a human pressing Enter within 5 s runs it without Lade" (`CHANGELOG.md`, 0.20.0, Upgrading). A command that runs without its credentials is not harmless: it falls back to whatever the environment holds, a stale token, a default kube context, a listener on the same port that leads to production.

The escape is for a person who can see the error and judge it, at a terminal, for a few seconds (the README promises 5 s). An agent, a script or CI never gets it: there is no one to judge, so silence cancels. Pre-exec cannot stop a shell from running the line it hooks, so it cancels by handing the shell an empty `PATH` for that one command; how is the pre-exec note's business. That cancel has a limit the README states: "A command typed with its full path still runs" (`README.md`, Tunnel), and so do a builtin and a shell function, which the shell does not look up on `PATH`.

A pre-tool hook that fails exits 1, and a harness that does not block on a hook error runs the line without Lade (`README.md`, Require Lade on Claude Code, `"onFailure": "block"`). So once the hook has a matched line, a step it cannot take, a key past its evaluation bound, a `config.json` it cannot read, a binding it cannot plan, does not fail the hook: it rewrites the line into the wrap, which takes the step again, names the error and exits 1 without running the line. A `lade.yaml` the hook cannot load matches nothing, and the hook does not fail there either: it answers each line that does not start Lade with the harness's deny, and pre-exec cancels the line, so every command of that tree is refused until the file is fixed or Lade is paused (owner answer, 2026-11-19), [y4zs3xgg4](config/discovery.md#y4zs3xgg4).

A disclaimer withholds access until a human approves (`README.md`, Patterns, Human approval). The architecture note used to describe pre-exec letting the withheld command run "without secrets" because "Fish `preexec` cannot cancel the main command". That reason is gone since 0.20.0 cancels in fish too, and running the command bare has the very risk the rule above names, plus a second run once `lade approve` replays it. A withheld command is cancelled on every door, with the limits above.

**§ <a id="t7kcndtra">t7kcndtra</a> Lade cancels a matched command whose access it cannot prepare.**

- (a) A failed secret, tunnel, pin, ticket or env conflict cancels the command on every door: `lade --`, a pre-tool wrap and `lade mcp` do not start it, and pre-exec cancels it too.

- (b) After a failed secret or tunnel, only a human at a terminal may run the command anyway, without Lade, by pressing Enter within a short window; silence, Ctrl-C or no terminal cancels it.

- (c) A command whose disclaimer withholds its access is cancelled as on a failure, in pre-exec too, until its approval code is given.

- (d) A pre-tool hook that has matched a line and cannot take a step of its access rewrites the line into the wrap, which fails it.

A pre-tool hook that cannot read its payload at all lets the tool call run and says so (`CHANGELOG.md`, 0.18.0, Unreadable `lade hook` payload): it never saw a command, so nothing matched. That rule belongs to the harness note.

## See which access was used

"Opt-in. `log: true` on a rule records that the command ran, what matched, which package resolved. Values are never stored" (`README.md`, Observability). The architecture note adds the exception: "Recording is opt-in on wrap paths. ... `lade eval` and `age-plugin-lade` are an access: they always write an `access` row" (`docs/architecture.md`, Observability). A wrap injects a value into a command the user typed; `lade eval` prints a value to whoever called it, which is the access a user most wants to see afterwards. Which rules vote on `log` is [kk9qxrvgt](observability/rows.md#kk9qxrvgt).

**§ <a id="gb0y44ydb">gb0y44ydb</a> Lade records which access a command used only when the repo turns recording on, except a value `lade eval` prints, which it always records.**

- (a) Recording is off by default: Lade writes a command's row only when a `log: true` vote reaches it.

- (b) `lade eval` always leaves a row naming the URI it resolved.

## Security model

Three promises make the access temporary in fact and not only in the process table. A value that lands in a file outlives the command; a hub any process can query is a secret server for every program the user runs; a disclaimer an agent can approve is no disclaimer.

Lade writes no value to disk. The diary stores names and sources, never values (`README.md`, Observability), and a command line that holds one is scrubbed first, [ec0ncyt0n](secrets/output.md#ec0ncyt0n). The ticket that carries a matched command from the pre-tool hook to the wrap holds the binding sources the wrap will resolve, and the 1Password service account as a URI, "not the token" (`docs/protocol.md`, Pre-event). Debug logs printed some resolved values until 0.20.0 removed those lines (`CHANGELOG.md`, 0.20.0, Secrets in debug logs); no code decides that absence, so a test must. The hub holds cached values in RAM only (`docs/cache.md`, What the hub holds).

**§ <a id="ezvgtge5j">ezvgtge5j</a> Lade keeps a resolved value in the memory and environment of its processes and the command's, and writes one to disk only as the `.file` output a rule asks for.**

- (a) A diary row names the command, the matched rules, and the bindings with their sources in place of their resolved values.

- (b) A ticket, which carries a matched command from one Lade process to the next, holds each binding's source in place of its value.

Values cross the hub's socket and sit in its RAM in plaintext, so the hub "serves only its own binary" (`CHANGELOG.md`, 0.20.0, Who may read the cache; `docs/cache.md`, Who may talk to the hub). The hub runs under the user's account, and its scope is that account: it serves the processes of its user, and the line it holds is the one between users (owner answer, 2026-11-19). Within the account, the peer check and the memory guard keep values to the hub's own binary for the programs of the user that ask.

**§ <a id="keyfb5k7w">keyfb5k7w</a> The hub serves the processes of its user and protects the values it holds against other users.**

A disclaimer is a rule's way to make a person look before access is granted. The approve code is not a secret, "an agent could recompute it" (`docs/architecture.md`, Disclaimer flow): it breaks the `LADE_APPROVE=1` reflex, it does not authenticate. So the guard is who is shown it: "A disclaimer no longer gives an agent the approve code: only a human approves, from a terminal" (`CHANGELOG.md`, 0.20.0, What an agent reads; `docs/protocol.md`, Disclaimer).

The guard is withholding, not proof. Lade checks the code, not who typed it, and the pre-tool hook keeps a `LADE_APPROVE=` prefix the harness writes. A harness Lade does not detect reads as a human without a terminal, and is shown the code with the box.

**§ <a id="b5862tcfn">b5862tcfn</a> A disclaimer tells a harness Lade detects that the access is withheld and that a human must approve it, in place of its approval code.**

## Exit codes

Agents and CI branch on how Lade exits, so the codes are a contract: "Keep documented exit codes (`src/exit_codes.rs`) stable across minor versions" (`AGENTS.md`, House rules), after the convention of "Patterns for AI Agent Driven CLIs" (`docs/architecture.md`, Exit codes). A wrap is transparent: a caller that ran `lade -- make test` must see `make`'s code, or it cannot tell a failing test from a failing Lade. That is why Lade's own codes are few and documented: any other non-zero code came from the wrapped command. A command that a signal ends has no exit code. Reporting it as 1, Lade's own "config / loader / generic error", hides an interrupted command behind a Lade failure; a shell reports 128 plus the signal, and the documented 130 for Ctrl-C is that convention (CHANGELOG, Unreleased, A command a signal ends). An MCP stdio server is a command Lade wraps too, so `lade mcp` reports it the same way.

**§ <a id="tk1e8j9nf">tk1e8j9nf</a> Lade's documented exit codes keep their meaning across minor versions.**

- (a) Lade exits 1 when its config or a loader fails, 3 when a disclaimer withholds access from a caller that cannot answer its prompt, and 130 when the user interrupts it.

- (b) A command Lade wraps, a `lade mcp` stdio server included, exits Lade with its own code, unchanged, or with 128 plus the signal's number when a signal ended it. A stdio server that still exits before `initialize` once its startup window is over fails `lade mcp` instead, [hkqw5dexk.a](harness/mcp.md#hkqw5dexk).

- (c) Link failures exit 4 (link down), 5 (a ref or tunnel the laptop did not resolve) and 6 (a walk no `lade connect` resolved), [d0jxpxadk](remote.md#d0jxpxadk).

The list is not every code Lade itself can exit with. A human who answers the interactive disclaimer prompt with anything but `yes` gets 1, since that is a refusal at a terminal, not the fail-closed 3 the architecture note reserves for a caller that cannot answer (`docs/architecture.md`, Exit codes). A command line clap cannot parse exits 1, as any error that reaches `main`; no doc gives it a code of its own. A stdio server that fails to start at all before `initialize` exits 1 as well, since no server status exists to pass on, and a client that leaves while Lade waits to restart a server that exited gets that server's own code.
