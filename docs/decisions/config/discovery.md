# Finding the configuration

A command needs access, and the repo that owns the command says which. Lade reads that from `lade.yaml` files, one per folder that owns commands, and from one per-machine file, `config.json`, that names the Lade user. The same files serve a human at a terminal and a harness, through pre-exec, pre-tool, `lade -- <cmd>` and MCP alike, so the rules that turn files into bindings must be one set, read the same way on every path.

The problem is that several files and several audiences meet on one command. A repo root, a subfolder and the home directory can each hold a `lade.yaml`. A rule can be meant for a human only, or a harness only. A value can differ per teammate. A value can be built from another one that must never reach the process. Each of those is a place where a silent misread hands the wrong secret to a command, or none, and the user cannot see which. These notes bind how the files are found and how they overlay, here, which bodies apply, [matching](matching.md), how a binding picks its value, [bindings](bindings.md), and how the CLI edits the files, [editing](editing.md).

What the command line is, after pre-tool peels `cd <dir> &&` and Lade's own prefixes, and how the audience is detected, belong to the harness notes. What a `ttl:` or `max_cache` does to the hub belongs to the cache notes.

## Discovery

The README section "Config" says Lade walks from the current directory up to `$HOME`, and "Hierarchy" says `~/lade.yaml` is a parent like any other. `docs/architecture.md` section 2 adds that parents above `$HOME` are not read: a file in `/` or `/Users` is not the user's, and could hand any command a value nobody in the repo wrote. A walk that starts outside `$HOME`, such as in `/tmp`, still stops short of every directory above `$HOME`. With no home at all, which the OS password database leaves only to a user it does not list, and a `LADE_CACHE_DIR` that lets Lade start, the walk reads up to `/`.

Two names for one file in one folder would make the nearest file ambiguous. CHANGELOG 0.19.0 made it an error, and the box says which to keep.

A walk that loses one file silently drops that file's rules, so a parent's `~` or a subfolder's override would vanish without a word. Every reader of the walk refuses it whole instead, with the box "Could not parse a lade.yaml."; `lade status` reports the same error with `ok: false`. A line's leading `cd <dir> &&` moves the walk once Lade has started, and a file there that does not load fails the command as a preparation error: `lade --` shows "Could not prepare the command.".

Every door then refuses the command (owner answer, 2026-11-19): a walk that does not load says nothing true about the access a command needs, and running it bare is the risk [t7kcndtra](../promises.md#t7kcndtra) names. `lade --` and `lade mcp` do not start it. Pre-exec cancels the line as any refusal, [gvtm9gyed.a](../wrap/cancel.md#gvtm9gyed), and the pre-tool hook gives the harness its deny answer, [j0my6hfhx.d](../harness/pretool.md#j0my6hfhx), for the directory it starts in and for the one a leading `cd` moves it to. The cost is every command of the tree, a terminal editor that would fix the file included, so Lade's own lines stay open. In pre-exec the hook runs a `lade ...` line as typed, and also `eval "$(lade off)"`, the line that pauses Lade in that shell, [vqrcg1d6p.a](../wrap/preexec.md#vqrcg1d6p). The pre-tool hook lets a line that starts Lade run: a verb such as `lade status` loads the walk itself and reports the error, and `lade --` refuses its command. A harness's own file edit is no shell line, so an agent can still fix the file. The message names the file, the error and the way out. A key that is not a valid regular expression is checked file by file, so its error names its file too. A payload with no command and an MCP tool call keep their own rules, [f7eb9ymg0](../harness/pretool.md#f7eb9ymg0) and [hk9h1sqyt.e](../harness/pretool.md#hk9h1sqyt): neither holds a line to refuse.

**§ <a id="y4zs3xgg4">y4zs3xgg4</a> Lade reads every `lade.yaml` or `lade.yml` from the directory a command runs in up to and including `$HOME`, where the walk stops.**

- (a) A walk that starts outside `$HOME` stops before the first directory that contains `$HOME`.

- (b) A directory that holds both `lade.yaml` and `lade.yml` fails the load.

- (c) A file on the walk that cannot be read or parsed fails the whole load: no rule of the walk applies, and Lade reports the error with the file that holds it.

- (d) While the walk does not load, every door refuses the command it was given: `lade --` and `lade mcp` do not start it, pre-exec cancels the line, and the pre-tool hook answers the harness with its deny.

- (e) While the walk does not load, the pre-tool hook lets a line that starts Lade run as sent, so Lade's own verbs still run there.

- (f) A refusal for a walk that does not load names the error, the file and the way out: fix the file, and in pre-exec, pause Lade with `eval "$(lade off)"`.

## Overlay

The README "Config" and "Hierarchy" sections: the nearest file wins on a key, the parent is the default, and YAML `~` on a key cancels a parent value. A command in `app/` uses `app/lade.yaml` over the repo root. The tests `test_build_nested_dirs_parent_first`, `test_collect_hydrate_null_cancels_earlier_key`, `test_later_network_replaces_secret` and `test_later_secret_replaces_network` hold this.

A key is one binding whatever its family: a child that turns a parent's secret into a tunnel replaces it, so a name never reaches the process twice. `NAME` and `.NAME` are two keys for one name, and neither replaces the other: both applying fails the command, [dyat01nyj.a](bindings.md#dyat01nyj). The loader marks in `docs/cli.md` tell a human at a terminal, "while secrets hydrate", that a closer file changed what the parent said, "A closer `lade.yaml` replaced a parent binding": a secret is marked whatever the family of the binding it replaced, and a tunnel's progress line carries no mark, since the source scopes the marks to secrets.

**§ <a id="sn2x1h4v9">sn2x1h4v9</a> Rules apply from the farthest file on the walk to the nearest, so the nearest file wins on a key and a parent's value is the default.**

- (a) Within a file, patterns apply top to bottom, and the bodies of one pattern in list order.

- (b) For one key, the last body that applies sets the binding, whatever its family, and a YAML `~` removes the value an earlier body set.

- (c) While secrets resolve, a human's progress line marks `(o)` on a secret that replaced a parent file's binding of its key, and `(u)` on a secret a body cancelled.

- (d) A `~` on `NAME` or `.NAME` removes the binding of that name in every family, and only what an earlier body set.

A `~` "cancels a parent value" (`README.md`, Hierarchy), so it names what came before it. A rule body is a YAML map, which has no order once read, so `TOKEN: ~` beside `.TOKEN: child` cancelled the child or not by chance, and the public parent `TOKEN` then collided with `.TOKEN` or did not. Reading a body's cancellations before its values makes that body mean one thing: the parent's `TOKEN` goes, and the private one stands. `NAME` and `.NAME` are one name, [dyat01nyj.a](bindings.md#dyat01nyj), so a `~` on either cancels it, a tunnel, a pin or a package as well as a secret. Tests: `a_cancel_beside_a_value_of_its_name_cancels_only_the_parent_value`, `a_private_cancel_removes_a_public_binding_of_its_name_in_every_family`.

`docs/cli.md` (Loader marks) defines `(o)` as "A closer `lade.yaml` replaced a parent binding", so that is what (c) binds. The progress line also marks a secret that replaced an earlier body of the same file; no doc names that case.

## The version pin

CHANGELOG 0.19.0 and the README "Config" section: a file may pin the Lade it needs with an empty YAML key, `: >=0.18.0`. Below the range, Lade refuses and shows the box. A repo that starts using a new key must be able to say so, or an old binary misreads the file and hands out the wrong access. There is no pin command: the range is edited by hand.

An old binary must still be able to fix itself. The README says `lade upgrade` still runs when this binary is too old, so the upgrade verb never reads the walk.

**§ <a id="bvr6wk206">bvr6wk206</a> A `lade.yaml` may pin the Lade versions it needs as a semver range on its empty key, and a binary outside that range refuses the file.**

- (a) The pin is a first line `: <range>`, or the empty key. An empty pin, a range that is not semver, or two pins in one file fail the load.

- (b) Every file on the walk is checked by every verb that reads it, `lade add` and `lade remove` included, and the refusal names the file, the range, and `lade upgrade`.

- (c) `lade upgrade` runs whatever the version pins on the walk say.

## The empty key

The empty key holds the version, so it cannot also be a rule. CHANGELOG 0.19.0 and the README say an empty command regex is not a rule: use `.` to match every command. An empty regex would match every command without saying so, and a pin line typed by mistake as a rule header would inject everywhere.

**§ <a id="jj7875tr3">jj7875tr3</a> The empty key of a `lade.yaml` holds only the version pin, and `.` is the rule for every command.**

- (a) An empty key holding a mapping or a list, or a `: ` line followed by an indented body, fails the load with a message that points to `.`.

## The machine file

`docs/env.md` and AGENTS.md: `config.json` holds the Lade user and `max_cache`, and `LADE_CONFIG_PATH` overrides its path. CHANGELOG 0.20.0 dropped the version-check state from it. AGENTS.md: `max_cache` is kept raw, so a bad value never stops `user` from loading, and unknown keys are written back for older binaries, since Lade 0.15 fails to load the file without its `update_check` and older binaries still share it. A machine can run several Lade versions at once, one per repo pin, and none may break the file for another.

**§ <a id="pq5ebbcs0">pq5ebbcs0</a> `config.json` is Lade's one per-machine setting file, and the only keys Lade itself reads from it are `user` and `max_cache`.**

- (a) `max_cache` is kept raw on load, so `user` loads whatever `max_cache` holds.

- (b) A key this binary does not read is written back untouched.

- (c) The file is at `LADE_CONFIG_PATH` when that is set, else `config.json` in the OS local config directory.
