# Diary rows

Lade's third promise is "a way to see which access was used" (`README.md`, title). The diary is that way: a local SQLite file where a command that matched rules leaves one row naming what it got, `lade log` and `lade usage` read it, `lade log share` hands a slice of it to someone else, and `lade log verify` says whether anyone edited it. When the diary records at all is [gb0y44ydb](../promises.md#gb0y44ydb), that it holds no value is [ezvgtge5j](../promises.md#ezvgtge5j), how a command line is scrubbed before it is stored is [ec0ncyt0n](../secrets/output.md#ec0ncyt0n), and the `cached` mark on a binding the hub served is [cj4kjwcp7](../cache/scope.md#cj4kjwcp7). These notes decide the rest: this one which flag turns a row on, what kind of row each door leaves, when it is written and what a row holds, [chain](chain.md) the chain that makes an edit visible, and [reading](reading.md) what a query reads and what a pack carries. [Status](status.md) decides the reporting surface around it: `lade status`, whose JSON agents and CI read, and `lade bench`.

The problem is that the diary audits the person whose machine holds it, on the path of every command they run. An audit row written after the command finished is lost when the user presses Ctrl-C; a diary that fails can fail a deploy; a diary any editor can rewrite without a trace is a story, not a record; a query run in one repo that mixes in every other repo's commands answers the wrong question; and a pack sent to a colleague must carry neither the sender's user name nor the paths of their machine. `lade status --json` is read by scripts and harnesses, so a key that moves breaks them silently, and an `ok` that turns false for something optional fails every CI job that checks it.

## Turning a row on

`log:` is last-wins and "Absent `log` is not a vote. `log: false` punches a hole on the rules it sits on" (`docs/observability.md`, Turn it on). A home `lade.yaml` with `.: .: log: true` records every command under it, and a child rule `"^git status": .: log: false` silences only `git status`, not `echo hi` (same section; `tests/log_emit/overlay.rs`, `overlay_log_parent_then_child`, `log_false_writes_nothing`). The table there decides which rules vote: those that matched, else, for a command no rule matched, every rule the walk loaded (`docs/architecture.md`, section 8; CHANGELOG 0.18.0, T protocol). A line the pre-tool hook records without a wrap, such as `cd`, and an MCP verb follow the same vote (`hook_cd_uses_the_matched_rules_log_not_the_walk`). The walk's vote counts only rules that apply to the audience, so a `when: human` `log: true` does not record a harness's unmatched commands (`src/config/collect.rs`, `log_on_walk_for_ignores_other_audience`). `lade eval` and `age-plugin-lade` resolve a URI with no rule to vote, and always record, [gb0y44ydb.b](../promises.md#gb0y44ydb).

A human who presses Enter to run a command without Lade after a provider failure gets no row (`docs/observability.md`, What gets a row): Lade handed that command nothing, so there is no access to record, whatever `log` says.

**§ <a id="kk9qxrvgt">kk9qxrvgt</a> Except on `lade eval` and `age-plugin-lade`, which always record, and on a command a human runs without Lade after a provider failure, which leaves no row, whether a command is recorded is decided by the last explicit `log` among the rules that matched it, or, when none matched, among the rules the walk loaded.**

- (a) Only a rule with an explicit `log` votes, and a later `log: false` on a matching rule turns recording off for that command only.

- (b) For a command no rule matched, only the rules whose `when` applies to its audience vote.

- (c) A laptop records each event of a link to an attached remote whatever the `log` of any rule, the remote's included, [b4tp4hvxk](../remote.md#b4tp4hvxk).

## What gets a row

"Shell wrap, inject, set, approve: `access` after hydrate, `denied` if a disclaimer is withheld, `seen` if nothing matched" (`docs/observability.md`, What gets a row). A row's kind tells a reader what Lade did, so `lade log --kind denied` lists exactly the commands a disclaimer stopped (`tests/log_emit/write.rs`, `disclaimer_denied_writes_kind`). A match that prepared nothing, such as a rule that only sets `log`, a login shell pinned for another command, or a setup package on `.`, is `seen` on every door (`hook_log_only_match_writes_seen_without_ticket`, `a_match_that_prepares_nothing_is_seen_on_every_door`).

One command, one row. "Pretool handler, match that wraps: no (the wrap writes)" (same table): the hook only plans, and the wrap it starts is the one that knows whether access was prepared. An MCP call is not a command Lade runs: "`tools/call` leaves a `seen` row without wrapping the verb" (CHANGELOG 0.18.0, MCP verbs and diary argv), and "Each later `tools/call` is a separate verb row, whether or not that prefix was used" (`docs/observability.md`). Some harnesses fire two hooks for one MCP call; the second fills the row of the first, found by the call's `tool_use_id` among the last 5 minutes of rows, a bound set because a lookup over the whole diary took 1.3 s under the write lock on 120,000 rows (CHANGELOG 0.20.0, MCP verb rows). When another row was appended in between, the chain forbids editing the first row, and the second hook appends a full copy. A documented promise wins, one verb row per call, and the diary stays append-only: the readers count a `tool_use_id` once, its newest row standing for the call. Which harness fires which hook is a harness detail, not a law.

`lade eval` and `age-plugin-lade` always leave an `access` row, [gb0y44ydb.b](../promises.md#gb0y44ydb). `unset`, `status`, `bench`, `log`, `usage`, `on` and `off` leave none (same table).

**§ <a id="bvttr6qk1">bvttr6qk1</a> A recorded command leaves a row whose kind says what Lade did: `access`, `denied` or `seen`.**

- (a) A row is `access` when Lade prepared a binding for the command, and `seen` when no rule matched or the rules that matched prepared nothing.

- (b) A command a disclaimer withheld leaves a `denied` row naming the matches it would have got, on a wrap, in pre-exec and in `lade mcp`.

- (c) A command a pre-tool hook wraps gets its row from the wrap alone.

- (d) Each MCP `tools/call` a harness hook reports leaves a `seen` row with `via` `mcp`, whether or not its server runs behind `lade mcp`.

- (e) `lade log` and `lade usage` count each `tool_use_id` once: when one call left two rows, its newest row stands for it.

- (f) A laptop records each event of a link to an attached remote as a `remote` row, [b4tp4hvxk](../remote.md#b4tp4hvxk).

## When a row is written

The row records that access was handed to a command, so it is written once the command has it, and not after the command ends. "A wrap ... writes its row on that connection once the child has started, or failed to start, and before it waits for the child. The child's exit status never changes the row: a failing command keeps it. ... Ctrl-C does not [lose it]: Lade forwards it to the child and finishes the write. Pre-exec `set` writes its row before the shell runs the command" (`docs/observability.md`, What gets a row; `tests/log_emit/write.rs`, `a_failing_command_keeps_its_access_row`, `a_command_that_cannot_start_keeps_its_access_row`). The window where Lade killed between the start of the child and the commit loses the row is a few milliseconds, and the docs state it as a limit. Writing after the output pump starts keeps a slow diary from holding the command's output back (CHANGELOG 0.20.0, Faster wraps).

The diary is a record, not a gate: "A write failure never changes the command's exit" (`docs/observability.md`, Turn it on; `write_failure_does_not_change_exit`). Every Lade process on the machine appends to the same file, so two of them opening it at once, even creating it, must both keep their rows (CHANGELOG 0.19.0, Diary first open; 0.18.0, Diary writers; `two_writers_do_not_corrupt`, `concurrent_first_open_keeps_every_row`).

**§ <a id="mzce8bpjn">mzce8bpjn</a> A wrap writes its row once the command has started or failed to start, before Lade waits for it.**

- (a) On `lade --`, a pre-tool wrap and `lade approve`, the row is written after the child is spawned, or its spawn failed, and before Lade waits for it to end.

- (b) Pre-exec `lade set` writes its row before it hands the shell the command's environment.

- (c) A diary that cannot be opened or written leaves the command's exit code and output as they would be with no diary.

- (d) Two Lade processes that open or create the diary at the same time both keep their rows.

## What a row holds

"The row is an audit snapshot at write time" (`README.md`, Observability). It records the public side of the access, which a reader can check against the yaml and the vault: "Vault addresses (`op://…`, `kubectl://…`) are [stored]. `matches` is the public keys and URIs, not values" (`docs/observability.md`, top). `command` is argv0 and `argv` the rest, so `lade log --group command` counts by program and still prints the whole line (CHANGELOG 0.18.0, MCP verbs and diary argv; `docs/observability.md`, Shape). `via` and `audience` let a reader split a harness's commands from a human's (`docs/architecture.md`, section 4).

A binding names the package that ran it and its version, taken from the lock next to that yaml when the row is written, so a later `lade update` does not rewrite what a past command ran with (`docs/observability.md`, Shape: "Implied or explicit packages add `package` and, when the lock beside that yaml agrees, `version`. Winners only. `.NAME` is omitted"). The `agent` object is "free-form JSON on each event (`harness`, `model`, `session`, plus later keys). Hook payload wins over env. Missing or unknown fields stay absent. Empty objects are omitted" (CHANGELOG 0.18.0, Diary `agent` object): a guessed field is worse than none in an audit. The 1024-scalar cap on each string has no stated reason and is not a law.

**§ <a id="j1mbwadzc">j1mbwadzc</a> A diary row names the command, how it reached Lade, who ran it and the public bindings that matched, as they stood when it was written.**

- (a) `command` is the line's first word and `argv` the rest: a string array for a shell or `lade mcp` line, the tool's argument object for an MCP verb.

- (b) `via` is one of `preexec`, `pretool`, `mcp`, `organic` or `unknown`, and `audience` is `human` or `agent`.

- (c) Each match names the directory of its yaml and its rule, and each winning public binding by its key, its yaml URI and its family; a `.NAME` binding is left out.

- (d) A binding whose URI implies or pins a package names that package, and the version the lock beside its yaml holds when the row is written, if that version agrees with the pin.

- (e) `agent` holds the hook's metadata, with the environment's only for a field the hook did not give, and a field Lade cannot read is left out.
