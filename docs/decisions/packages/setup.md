# The repo verbs

`lade setup` wires a repo and `lade teardown` undoes it, and some packages are not a CLI but a thing installed into the repo. A verb that reaches past its repo, or a setup action that runs on a plain command, changes what a user never asked to change. This note binds when setup actions run and what each verb writes and removes.

## Setup actions

Some packages are not a CLI for a command but a thing installed into the repo: `apm://` and `skills://` are "Setup-rule package only. Not a command pin" (`README.md`, scheme table), and a pin may carry "Optional `?setup=` / `?teardown=` run on `lade setup` / `lade teardown` in this repo" (`README.md`, Package; the `dcg` example installs and uninstalls a guard). They change the repo, so they run with the repo's verbs and nowhere else: running one on every matched command would reinstall a package per command line. A setup package bound on a command rule would wrap the command with nothing, so Lade refuses it and says where it belongs: "Put apm:// and skills:// on the `.` rule, not on the command" (`command_package_is_refused`). A command matched by any rule but `.` fails on it, so the message names no other rule. The pre-tool hook sends such a command to the wrap, which refuses it, rather than let the harness run it bare (`a_setup_package_on_a_command_rule_goes_to_the_wrap`).

They run the locked CLI, `apm` and `skills` through their implied pins, for the same reason a pin does ([rtcp7pj7e](pins.md#rtcp7pj7e)). A setup step that fails is a repo that is not set up, so the verb fails with the tool's error. Outside a git repo there is no root to run them in, so setup and teardown run none (`run_outside_git_does_nothing`).

**§ <a id="vwj3wvrzs">vwj3wvrzs</a> Setup packages and `?setup=` / `?teardown=` commands run only on `lade setup` and `lade teardown`, at the git root, with the locked CLI.**

- (a) An `apm://` or `skills://` binding on any rule but `.` fails the command it matches.

- (b) Setup installs each setup package with the locked `apm` or `skills` CLI, and teardown removes it.

- (c) `?setup=` and `?teardown=` on a pin run that pin's locked binary with those arguments.

- (d) Setup and teardown find the git root of the current directory first, and run every setup package and `?setup=` / `?teardown=` command there.

- (e) A pin it cannot parse, a missing locked CLI, or a command that exits non-zero fails the verb.

## `lade setup`

"Setup installs a **shell hook** (pre-exec) once, in your profile, so every command you type can match. The rest stays in the repo: pre-tool hooks ... and the lock for pinned packages" (`README.md`, Getting started). "The shell wrap is written only the first time this machine has no pre-exec. Later, a missing wrap is `lade hook enable --shell`. `CI` set skips the wrap" (CHANGELOG 0.19.0, `lade setup` / `lade teardown`). A user who removed the wrap from one profile on purpose must not get it back from every `lade setup`, and a CI job has no interactive shell to wrap. Setup reads `CI` before it looks for the shell that started it, so a CI step that no known shell started still wires the repo (`ci_skips_shell_wrap_before_reading_the_parent_shell`). The rest of setup needs no shell either, so a parent that is no shell with a profile hook, such as tcsh, ksh, `sh` or an IDE task, gets a line that names it and the way to enable the hook from bash, zsh or fish, and setup goes on to wire the repo (`setup_from_no_shell_lade_knows_names_it_and_wires_the_repo`). A profile Lade cannot read, a byte that is not UTF-8 or a file it may not open, cannot tell whether the machine has the hook, so setup writes none, names the file, leaves it as it is, as [aq4rxhrqa.c](../wrap/preexec.md#aq4rxhrqa) does for `lade hook enable --shell`, and goes on to wire the repo (`setup_with_a_profile_it_cannot_read_writes_no_hook_and_wires_the_repo`). A profile that holds Lade's line answers for the machine whatever another one reads. `lade status` and the reload hint after `lade upgrade` still read such a profile as having no hook.

Outside git there is no repo to wire: "Outside git, the repo plane is refused" (`docs/architecture.md`, Installing pre-tool), and setup says "Packages, locks, and pre-tool hooks are skipped". `$HOME` is the one place a lock may live without a repo, for `~/lade.yaml` ([a45aqmr7m.b](lock.md#a45aqmr7m)). Where setup writes the pre-tool hooks is [rkpfgpx72](../harness/install.md#rkpfgpx72).

**§ <a id="h528pmcm2">h528pmcm2</a> `lade setup` wires this git repo, and writes the shell hook only on a machine that has none.**

- (a) Setup writes the shell hook when no profile of the machine has Lade's pre-exec or fails to read, `CI` is unset, empty, `0` or `false`, and bash, zsh or fish started it; otherwise it reports this shell's hook current or missing, an unreadable profile, that CI skips the wrap, or the shell it cannot hook, and goes on with setup.

- (b) Outside a git repo, setup installs packages and writes a lock only in `$HOME`'s own mise plane, when it runs in `$HOME`.

## `lade teardown`

"`lade teardown` is this repo only: pre-tool hooks and `?teardown=` on setup packages. It does not remove the shell wrap" (`README.md`, Package). The shell hook serves every repo on the machine and the home hooks every repo of a harness, so a repo's teardown must not take them (`docs/cli.md`, `lade teardown`). The lock and the yaml are committed files: teardown is about this machine's wiring, not the repo's content. The shell hook, the home pre-tool hooks, the lock, the yaml and the installed CLIs therefore stay as they are.

`--global` is the other store: "On-disk isolation (mise, tickets) | `lade teardown --global`" (`docs/cli.md`, Three stores), "wipe this machine's on-disk cache (mise isolation, tickets, sidecars). Not the RAM hub. Home pre-tool hooks stay until `lade hook disable --global`" (`docs/cli.md`, `lade teardown`). Everything in that cache is rebuilt by the next setup or wrap; nothing in it is truth.

**§ <a id="ep7nzy2xg">ep7nzy2xg</a> `lade teardown` undoes only this repo's setup: its pre-tool hooks and its teardown actions.**

- (a) Teardown removes this repo's project-scope pre-tool hooks, runs each pin's `?teardown=` command, and removes each setup package.

- (b) `lade teardown --global` deletes this machine's Lade cache on disk alone, and keeps every hook.
