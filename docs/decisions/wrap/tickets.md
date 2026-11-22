# Wrap and tickets

Three of Lade's doors start the command themselves: `lade --` (and its alias `lade <cmd>`), the pre-tool wrap, which is `lade --pretool=<id> '<line>' && true`, and `lade approve`. The fourth cannot. A shell hook runs `lade set` before the line and `lade unset` after it, and the shell runs the line in between, with whatever Lade exported (`docs/architecture.md`, High-Level Flow). Between the process that matches a command and the one that runs it, a ticket (T) carries the plan (`docs/protocol.md`).

Around that core the wrap does six more things: it hands the pre-exec shell its environment and takes it back, cancels a command it cannot prepare, lets a human run it anyway, gates a disclaimer behind a person, stands in for the child for signals and exit status, and talks on stderr to two very different readers, a person at a terminal and a harness that reads every byte as context.

The problem is that each of these spans several processes and three shells, each with its own limits. No shell hook can stop the line it hooks. fish aborts its pre-exec hook on SIGINT and runs the command anyway (`docs/tunnels.md`). macOS's `/bin/bash` 3.2 has no command-not-found handler. A mechanism that holds in bash and leaks in fish, or holds for `lade --` and not for `lade set`, is a command that runs without its access, or with another command's. The promises these mechanisms keep are in [promises](../promises.md): one command then gone, the same wrap for every door, fail closed, only a human approves, stable exit codes. These notes state how: this one the ticket that carries a plan between processes, [pre-exec](preexec.md) the shell's environment, hook and profile line, [cancel](cancel.md) cancelling, running anyway and interrupting, [disclaimers](disclaimers.md) the approval gate, and [child](child.md) the shell the command runs in and how Lade exits.

## The ticket

"The file is the pre-event: everything needed to run providers. No second `lade.yml` walk. No rematch. Missing file: walk once, as a cold start" (`docs/protocol.md`). The pre-tool hook matches a line, writes the ticket and rewrites the line to the wrap, which reads the plan back. Pre-exec `set` writes one on every match, so `lade unset` can release the command's tunnels and `lade approve` can find a withheld command. Both plan through one shared planner, [g6sqmp8tb.a](../promises.md#g6sqmp8tb), and a ticket holds sources, never values, [ezvgtge5j.b](../promises.md#ezvgtge5j).

A ticket is the access of one command. The pre-tool hook leaves a finished wrap alone only when its ticket holds that very line (`ticket_finished`, `src/pretool/classify.rs`), and an approval code is bound to the ticket's command. A direct `lade --` ignores a leftover `LADE_T` (`docs/protocol.md`, Pass; test `direct_inject_ignores_leftover_lade_t`). The wrap and `lade set` hold to the same rule: without it, a pending pre-exec ticket still in `LADE_T` would hand the next line the plan of the last one, its disclaimer or its secrets.

A pre-tool ticket may have been written for another directory than the one the wrap resolves. It "still supplies the secrets, but skips the cache: a body is keyed by the rule that holds it, and a value must never land in another rule's" (`docs/protocol.md`, Working directory).

The child never sees the ticket id or `LADE_T` (`docs/protocol.md`, Pass). `LADE_RESTORE` "may contain secrets" (`docs/env.md`, Internal protocol), and a leftover `LADE_VIA` is ignored and never inherited (CHANGELOG, Organic versus unknown via). A child that saw them could act as a Lade process it is not, or read the previous values of the variables Lade replaced.

**§ <a id="nv6nfbbs2">nv6nfbbs2</a> A ticket carries the access plan of one command from the Lade process that matched it to the one that runs it.**

- (a) A wrap or `lade set` uses a ticket only when it exists and holds the same command; otherwise it walks the yaml, as a cold start.

- (b) A ticket written for another directory supplies its secrets, and the wrap fetches them from their providers with the hub cache set aside.

- (c) Lade removes `LADE_T`, `LADE_RESTORE` and `LADE_VIA` from the environment of the command a wrap runs and of an MCP server it starts.

A provider CLI and a tunnel helper inherit Lade's own environment. Under a pre-exec command, which holds `LADE_T` and `LADE_RESTORE` itself (see [qmybbcmkx](preexec.md#qmybbcmkx)), a nested `lade --` hands them to its providers too; the sources promise only that the child does not see them.

A ticket id shares the command line with the command. `lade --pretool echo` is a command named `echo`, four letters like an id, so "a 4-char token after `--pretool` is an id only when that file already exists", and an id is regenerated "if the id is `eval`, `help`, `hook`, or `user`", Lade's four-letter verbs (`docs/protocol.md`): an id is four characters, so those four are the verbs it could spell, and a new four-letter verb joins the list. Two hooks can write at the same instant, so the file is created exclusively and a collision draws a new id ("Regenerate the id if `create_new` fails").

**§ <a id="ac9463jhq">ac9463jhq</a> Each ticket id names its own ticket's file, and Lade tells it apart from a command and a Lade verb.**

- (a) Only a `--pretool` among Lade's own options, before `--` and the command, names a ticket: a separate word after it is a ticket id only when that ticket exists, and `--pretool=<id>` always names one.

- (b) Lade redraws a new ticket id that spells a Lade verb.

- (c) A new ticket is created as a new file, and an id whose file exists is redrawn.

A shell started inside a pre-exec command inherits that command's `LADE_T`, and the outer ticket is still in use: its `lade unset` releases its tunnels once the outer command ends. So `lade set` takes as its own only a pending ticket, the withheld line of its own shell, which the next line's ticket replaces; any other `LADE_T` it drops from the shell and leaves its file alone (test `a_nested_set_leaves_the_outer_commands_ticket`). When that `lade set` fails before it prints, on a cache directory Lade cannot use, the outer `LADE_T` reaches the inner `lade unset`, which takes as its own only a pending ticket or one written for its line (test `a_nested_unset_leaves_the_outer_commands_ticket`); an inner line spelled exactly as the outer command still reads as its own.

A ticket outlives its reader only as long as cleanup needs it. "Read once: a long command must not hold its ticket until it exits" (`src/wrap/run.rs`; test `a_running_command_no_longer_holds_its_ticket`). A pre-exec ticket must stay, since `lade unset` reads it after the command to release its tunnels: "Pre-exec T stay: a long command needs its own until `lade unset`" (`docs/protocol.md`, Pass). A pre-exec command Lade cancels gets no `lade unset` (see [gvtm9gyed](cancel.md#gvtm9gyed)), so `lade set` deletes its ticket itself. A pre-tool ticket no wrap read within an hour "never will be: the harness denied the command, another hook blocked it, or the line never ran Lade. Each new pre-tool T sweeps those, and any T that does not parse" (`docs/protocol.md`, Pass).

**§ <a id="gy20cgb72">gy20cgb72</a> A wrap deletes the ticket it reads, and a pre-exec ticket lives until the `lade unset` after its command.**

- (a) A wrap deletes the ticket it read as soon as it has read it, before the command runs.

- (b) `lade unset` deletes the ticket `LADE_T` names and releases the tunnels leased under it.

- (c) `lade set` deletes the ticket of a pre-exec command it cancels, unless a disclaimer withheld it.

- (d) Each new pre-tool ticket deletes every ticket past a fixed age that is a pre-tool ticket or does not parse, and keeps every pre-exec ticket.

- (e) A ticket in `LADE_T` that is neither pending nor written for the line at hand belongs to an outer shell's command, and `lade set` and `lade unset` leave its file, its outputs and its tunnels to that command.

The age is an hour (`docs/protocol.md`, Pass); no source gives a reason for that value, only for the sweep. A command whose disclaimer pre-exec withheld keeps its pending ticket until `lade approve` reads it, under (a), or until the `lade unset` after the next line, under (b). The approval therefore comes right after the withheld command.
