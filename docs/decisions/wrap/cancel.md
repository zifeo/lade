# Cancelling, running anyway and interrupting

A command whose access cannot be prepared must not run, yet a shell hook cannot stop the line it hooks, a person may want to run it without Lade, and Ctrl-C can land while Lade is still preparing. This note binds how each door cancels, what running anyway means, and what Ctrl-C stops.

## Cancelling a command

A failed secret, tunnel, pin, ticket or env conflict cancels the command on every door, [t7kcndtra](../promises.md#t7kcndtra). `lade --` cancels by not starting it. A shell hook cannot stop the line it hooks, "so `lade set` prints code that sets `PATH` to an empty directory and erases `LADE`, so the hook skips `lade unset`, which has nothing to undo. The command, its aliases, and the functions that wrap it find nothing to run, and the shell's command-not-found handler prints `Lade cancelled <command>.` ... `PATH` and the user's handler come back before the next hook in zsh and bash, and before the prompt in fish" (`docs/tunnels.md`; `README.md`, Tunnel). A command withheld by a disclaimer is cancelled the same way, [t7kcndtra.c](../promises.md#t7kcndtra).

Two limits are stated and stay: a command typed with its full path still runs, and bash before 4.0, such as macOS's `/bin/bash`, prints its own `command not found` instead of Lade's line. A third follows from the mechanism: a line that needs no external command, a builtin such as `cd`, `export` or `source`, or a function made of them, still runs, and no line says it was cancelled. `sh` has no pre-exec hook, so there is nothing to cancel there.

A `lade.yaml` that does not load is a refusal like any other: `lade set` drops the previous snapshot, prints the error and the way out, and cancels the line (owner answer, 2026-11-19), [y4zs3xgg4](../config/discovery.md#y4zs3xgg4). A `lade set` that fails before it knows its shell exits 1 without the cancel, and the shell runs the line with nothing from Lade: a cache directory Lade cannot use, or a parent shell Lade cannot name, for which there is no shell to write the cancel in.

**§ <a id="gvtm9gyed">gvtm9gyed</a> Pre-exec cancels a command by giving the shell, for that one command, a `PATH` with nothing to run.**

- (a) Every `lade set` that refuses a command once it knows its shell prints this cancel in place of its exports.

- (b) A command the shell looks up on `PATH` is not found, the shell says `Lade cancelled <command>.` through its command-not-found handler, and it gets its `PATH` and its own handler back before the next command.

- (c) The cancel erases the hook's `LADE`, so the hook skips `lade unset` for the cancelled command.

## Running anyway

After a failed secret or tunnel, a human at a terminal gets 5 s to press Enter and run the command "anyway, without Lade" (`README.md`, Tunnel), [t7kcndtra.b](../promises.md#t7kcndtra). Without Lade means without any of it: "a human at a terminal gets 5 s to press Enter and run it without Lade, which writes no diary row" (CHANGELOG 0.20.0, A failure cancels the command). Half of the access, or a row claiming access the command never had, would be worse than none.

The answer is read from `/dev/tty`, "never stdin: that belongs to the command" (`src/wrap/mod.rs`, `handle_provider_failure`). Keys typed before the question are not an answer (`src/wrap/keys.rs`, `forget_typed`): an Enter typed ahead, while providers ran, must not run the command bare. The flush has a race: a key the terminal reader has already taken but not yet handed on when the flush runs is read as typed after the question.

**§ <a id="ntr20kvnb">ntr20kvnb</a> A command a human runs anyway after a failure runs as Lade runs a command no rule matches, outside the diary.**

- (a) A wrap that runs the command anyway starts it with the environment and `PATH` that `lade --` gives a command no rule matches, and skips the diary.

- (b) In pre-exec, `lade set` that runs the command anyway leaves the shell's environment as it was.

- (c) Lade reads the answer from the terminal it opens itself, and leaves stdin to the command.

- (d) Lade flushes the keys typed before the question, so only a key typed after it answers.

That environment is the one Lade started with, less what every wrap's child loses: Lade's protocol variables, `BASH_ENV` ([dpky46ff1.b](child.md#dpky46ff1)), `MISE_ENV` and `MISE_SETTINGS`, which Lade has dropped from every child since the mise pins arrived (CHANGELOG, #205) with no stated reason. In pre-exec a command run anyway keeps no ticket, so the `lade unset` after it deletes no file, a user's own file at a `.file` path included, [fz8r6d2ka.e](../secrets/output.md#fz8r6d2ka) (test `a_file_of_the_user_survives_a_run_anyway_and_the_unset_after_it`). A `LADE_APPROVE` that `lade approve` set to let its command through is Lade's own, so the command gets the value Lade started with instead (test `an_approved_command_gets_no_approval_code_lade_set_itself`).

## Interrupting

"Ctrl-C while tunnels connect stops them and cancels the command" (`README.md`, Tunnel). Lade must stop what it started, or a tunnel half-opened for a command that never ran stays leased. fish "aborts its pre-exec hook on SIGINT and runs the command anyway, so in fish, and during the 5 s question in every shell, Lade reads the terminal without signals and gets Ctrl-C as a key" (`docs/tunnels.md`). The exit code is the documented 130, [tk1e8j9nf.a](../promises.md#tk1e8j9nf).

**§ <a id="s6j22qtcr">s6j22qtcr</a> Ctrl-C while Lade prepares a command stops what Lade started for it and cancels the command, in every shell.**

- (a) Ctrl-C, `kill` or a hangup while secrets hydrate and tunnels start stops the tunnels being started, deletes the ticket, and cancels the command with the interrupt exit code.

- (b) In fish's pre-exec hook, from before the pins until the command is ready, and during the run-anyway question, Lade reads Ctrl-C from the terminal as a key, with the terminal's signals off.
