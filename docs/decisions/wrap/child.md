# The child and its exit

A wrap stands between the command and whoever started it: the line must run in the shell it was written for, the signals meant for it must reach it, and its exit status must come back unchanged. This note binds which shell runs the line, how Lade stays in front of it, and where each exit code comes from.

## The child

"Lade runs `cmd` in the shell that started it, its parent process: the shell the command was written for. An inherited variable never picks it" (`docs/protocol.md`, Shell). For a pre-tool wrap that is the harness's shell, which the rewrite keeps as Lade's parent, [bwgf7z8q5](../harness/pretool.md#bwgf7z8q5), so a pre-tool wrap with no shell parent was not started by the harness's shell, and running it in `sh` or in the shell `SHELL` names would run a line in a shell it was not written for (CHANGELOG 0.20.0, Pre-tool commands in the wrong shell; tests `pretool_without_a_shell_parent_fails`, `an_inherited_shell_variable_never_picks_the_shell`). "A direct `lade inject` or `lade approve` with no shell as parent runs its line in `sh`. A task runner (make, npm scripts, just) runs a line with `sh -c`, which execs Lade and leaves the runner as its parent: that line was written for `sh`" (`docs/protocol.md`, Shell). The child shell skips the user profile (`fish --no-config`, `zsh -f`, `$BASH_ENV` unset) "so startup files cannot overwrite a resolved secret" (`docs/architecture.md`, Execution). A mise shim standing in for the shell rewrites `PATH` before the script runs (`src/wrap/exec/mod.rs`, `spawn_child`; test `inject_keeps_path_through_a_shim`), so the script sets the wrap's `PATH` again first.

**§ <a id="dpky46ff1">dpky46ff1</a> A wrap runs the whole command line in the shell that started Lade, or in `sh` when no shell did, with the user's startup files skipped.**

- (a) A wrap other than a pre-tool wrap whose parent is not bash, zsh, fish, sh or dash runs its line in `sh`.

- (b) The child shell starts with its startup files off, as `fish --no-config` and `zsh -f`, and with `BASH_ENV` unset.

- (c) The child shell sets the wrap's `PATH` again before it runs the line.

- (d) A pre-tool wrap whose parent is not bash, zsh, fish, sh or dash fails, and a wrap picks its shell from its parent process alone, whatever `SHELL` or `LADE_SHELL` hold.

A wrap is transparent: a caller that ran `lade -- make test` must see `make`'s outcome, [tk1e8j9nf.b](../promises.md#tk1e8j9nf), and a tunnel stops with its command, so Lade must outlive it to clean up, [jfxr4rmae.c](../promises.md#jfxr4rmae). "Hangup is ignored. Stop signals end the wrapper. `USR1` / `USR2` / `WINCH` are forwarded" (`docs/architecture.md`, Execution; CHANGELOG, Child signals). Tests: `inject_ignores_hangup_and_keeps_child`, `mcp_ignores_hangup_and_keeps_child`. How Lade exits once the command ends, 128 plus the signal when one ended it, is [tk1e8j9nf.b](../promises.md#tk1e8j9nf).

A wrap that masks puts the terminal in raw mode, so the bytes a terminal sends in reply to a query do not echo (`src/wrap/exec/pty.rs`). Raw mode also turns a terminal's Ctrl-C into a byte: until 0.20.0 a masked command "ran with the terminal in raw mode, so Ctrl-C reached it as a key, not an interrupt, and a hung apply did not stop" (CHANGELOG 0.20.0, Ctrl-C with secrets). The interrupt stays on, and Lade forwards it under (a) (test `ctrl_c_interrupts_a_masked_command`).

**§ <a id="fe8gp3s51">fe8gp3s51</a> While a command Lade wraps runs, Lade forwards the signals meant for it, stays up until it ends, and exits as it did.**

- (a) Lade forwards INT, TERM, QUIT, USR1, USR2 and WINCH to the command and ignores hangup.

- (b) **Retired.**

- (c) At a terminal, Ctrl-C during a masked wrap is an interrupt, as without masking.

`lade --`'s child stays in Lade's terminal session, while an MCP stdio server gets its own session and process group (CHANGELOG, Child signals; tests `inject_child_stays_in_lade_session`, `mcp_child_gets_its_own_session`). That split is the MCP bridge's business.

## Exit codes

The table and its stability are [tk1e8j9nf](../promises.md#tk1e8j9nf). In this area each code has one source: 1 when Lade refuses a command, a pre-exec `set` included, when `lade approve` finds nothing to run, and when a snapshot cannot be restored, [qmybbcmkx.b](preexec.md#qmybbcmkx); 3 when a disclaimer withholds access outside interactive mode, [betd87z6r](disclaimers.md#betd87z6r); 130 when Ctrl-C or a stop signal cuts the preparation short, [s6j22qtcr.a](cancel.md#s6j22qtcr); and the command's own status otherwise, 128 plus the signal when one ended it, [tk1e8j9nf.b](../promises.md#tk1e8j9nf).
