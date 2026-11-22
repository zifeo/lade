# Pre-exec

A shell hook cannot start the command: it exports values before the line and takes them back after, and its stdout is code the shell evaluates. The shell must get its environment back, the hook must wrap the right lines, and the profile must load it without Lade touching anything else. This note binds the pre-exec environment, the hook, the profile line and the channel each reader is spoken to on.

## The pre-exec environment

In pre-exec the shell holds the command's values and gets its previous environment back after it, [jfxr4rmae.b](../promises.md#jfxr4rmae). `lade set` exports, next to the values, a snapshot of what each variable held before, `PATH` from before any pin included, in `LADE_RESTORE`; `lade unset` evaluates it back. `lade unset` is a child of the shell and reads the snapshot from its environment, so the snapshot is exported: the user's command, and what it starts, inherit it as they inherit `LADE_T`. Only the command a wrap runs never sees it, [nv6nfbbs2.c](tickets.md#nv6nfbbs2).

A snapshot left by an earlier command must not leak into the next set (`docs/architecture.md`, Disclaimer Flow, step 2): `lade set` drops it before anything else. A snapshot `lade unset` cannot decode cannot be restored, and silently skipping it would leave the command's secrets in the shell, so unset erases what it knows Lade set and says so (test `test_unset_corrupt_restore_fails`).

**§ <a id="qmybbcmkx">qmybbcmkx</a> In pre-exec, `lade set` hands the shell's previous values of the variables it sets to the `lade unset` after the command, in `LADE_RESTORE`.**

- (a) `lade set` unsets any snapshot an earlier command left before it prints anything else.

- (b) A snapshot `lade unset` cannot decode is an error: Lade unsets the secret and tunnel variables the ticket names and its own protocol variables, says so in a box, and exits 1.

One limit stays. A snapshot that cannot be decoded holds no previous value, so a variable that had one before the command is erased, not restored, and a ticket does not name a pin's variables, so they stay, a pinned `PATH` included. `lade unset` reads no `lade.yaml`: everything it undoes is in the ticket and the snapshot, so a yaml broken while the command ran still gets the shell its environment back (test `unset_restores_the_shell_when_lade_yaml_broke_during_the_command`).

The hook evaluates what `lade set` and `lade unset` print, so their stdout is shell code and nothing else (`docs/architecture.md`, Via, Audience, UI). A value that broke out of its quotes would run as code in the user's shell, or arrive cut (tests `values_round_trip_in_every_shell`, `test_set_escaping`).

**§ <a id="aknp07yqt">aknp07yqt</a> A value Lade hands a pre-exec shell reaches it byte for byte, quotes, backslashes and newlines included.**

## The hook

`lade on` and `lade off` "pause pre-exec in this shell" (`README.md`, Getting started); they print code for `eval "$(lade on)"` (`docs/cli.md`). The hook skips "any command that starts with `lade ` or is exactly `lade`, so `lade approve`, `lade status`, and `lade upgrade` do not wrap themselves" (`docs/architecture.md`, Pre-exec short-circuit). A wrapped `lade approve` would run its own `lade set` first, and a wrapped `lade upgrade` would replace the binary under its own hook. `eval "$(lade off)"`, the README's line that pauses Lade, runs `lade` inside the line, so a cancelled line could not run it: while a `lade.yaml` does not load, every wrapped line is cancelled, and this line is the way out the refusal names, [y4zs3xgg4](../config/discovery.md#y4zs3xgg4). Another spelling of it is still wrapped.

**§ <a id="vqrcg1d6p">vqrcg1d6p</a> The pre-exec hook wraps every line the shell runs except Lade's own commands, and only in a shell that evaluated `lade on`.**

- (a) The hook returns at once on a line that is `lade`, starts with `lade `, or is `eval "$(lade off)"`, so the shell runs it as typed.

- (b) `lade off` removes Lade's two hook functions from the shell that evaluates it and keeps every other hook that shell has.

The hook lives in three copies, `scripts/on.bash`, `scripts/on.zsh` and `scripts/on.fish`, with `scripts/off.*` beside them, embedded in the binary by `src/preexec/hook.rs`. bash gets `scripts/bash-preexec.sh` in front of its copy, since bash has no pre-exec event of its own. The bash copy is the reference; the zsh and fish copies say the same in their shells. Silcrow reads no zsh or fish, so only the bash copies carry the markers of this law. `eval "$(lade off)"` is itself a line the hook wraps, and it removes the after-hook, so the `off` code runs that after-hook first and the shell gets its environment back, [jfxr4rmae.b](../promises.md#jfxr4rmae). The hooks also skip a line that is exactly `source off.<shell>`, and the after-hook skips `lade unset` for `source on.<shell>`; no source gives the reason, so it is not a law. `lade on` prints the `off` code before the `on` code, which removes an earlier Lade hook first; no source gives the reason, so it is not a law.

## The profile line

"`lade hook enable --shell` installs pre-exec in this shell's profile", and `lade hook disable --shell` removes it (`README.md`, Getting started; `docs/architecture.md`). The line it writes runs `lade on` by name, "the PATH name, not the install path. A later mise or cargo bin is picked up on the next shell" (CHANGELOG 0.20.0, Shell wrap). The profile is the user's file, holding everything else their shell does, so Lade owns one marked line of it and nothing more. A profile Lade cannot read, a byte that is not UTF-8 in `.bashrc` or a file it may not open, used to read as empty, so enabling the hook replaced the whole file with Lade's line, and `lade setup`, which reads the same failure as no hook, did the same. A silent loss becomes an explicit failure, as for a harness config Lade cannot read, [e3ntx7tnt](../harness/install.md#e3ntx7tnt). A missing profile is not unreadable: Lade creates it. When setup writes the line is [h528pmcm2](../packages/setup.md#h528pmcm2).

**§ <a id="aq4rxhrqa">aq4rxhrqa</a> Lade owns one marked line of a shell's profile, the one that loads its pre-exec hook, and keeps every other line, and a profile it cannot read, as it is.**

- (a) `lade hook enable --shell` writes one line that runs `lade on` by its `PATH` name, and enabling again leaves one.

- (b) `lade hook disable --shell` removes only the marked line, and both keep every other line of the file.

- (c) A profile Lade cannot read fails the verb, names the file, and stays as it is.

## The UI channel

In pre-exec, stdout is the shell protocol, `export` and `unset` lines the hook evaluates, so `lade set` and `lade unset` never prompt and write everything meant for a person to stderr (`docs/architecture.md`, Via, Audience, UI), [aknp07yqt](preexec.md#aknp07yqt). What an agent reads on stderr, one `lade:` line per message with no box, colour or progress, is [kgqztjzym.d](../harness/audience.md#kgqztjzym). A person gets a bordered box when stderr is a terminal and bare lines otherwise. `MessageBox` for attention against `Report` for results is a house rule of presentation (`AGENTS.md`, House rules), with no check that refuses a stray `eprintln!`, so it stays guidance.
