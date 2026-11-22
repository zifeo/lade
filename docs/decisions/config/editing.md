# Editing the files

Users and scripts change `lade.yaml` from the CLI as well as by hand. An edit that rewrote the rest of the file, or a family name that meant something else in a script, would change a team's reviewed config behind its back. This note binds what `lade add` and `lade remove` touch, and how the CLI names a family.

## Editing the files

The README "Families" section and `docs/cli.md`: `lade add secret`, `lade add tunnel` and `lade add package` write the nearest yaml, then run `lade setup`, so a new pin is installed and a new provider's CLI is locked at once. `lade remove` drops a binding and does not run setup: teardown commands of a removed package run on `lade teardown`, not on an edit. Both keep the `:` pin (`upsert_keeps_version`), since editing the file must not change which Lade may read it. A `lade.yaml` is written by hand, with comments that say why a rule binds what it binds (owner answer, 2026-11-19), so the verbs edit it line by line instead of writing the file anew: a new binding is one line after the rule's last binding at their indent, a new rule is appended at the end, a changed value keeps its key as written and the comment beside it, and a removed binding takes its own lines, and the rule's key line when it was the last. The `@latest` write-back of setup ([h01jcjwrv](../packages/lock.md#h01jcjwrv)) edits the file the same way. Lade then reads the edited text back and compares it with the file it meant to write, so a layout it misreads fails the verb instead of reaching the disk. A failed edit must not leave a half change behind: `lade add` that writes a file no Lade can load, or `lade remove` that rewrites a file and then reports the binding missing, would change the file while saying it failed.

**§ <a id="qp8tmdhae">qp8tmdhae</a> `lade add` and `lade remove` edit a `lade.yaml` on the walk, the nearest by default, and only `lade add` then runs `lade setup`.**

- (a) With no file on the walk, `lade add` creates `lade.yaml` in the current directory.

- (b) Both keep the file's version pin.

- (c) `lade add` writes the edited file only after it loads with its rule keys compiled, and an edit that fails that check fails the verb.

- (d) `lade remove` of a binding the file does not hold fails without writing the file.

- (e) Both change only the lines of the binding they edit, and the rule's key line when they add or drop the whole rule, so every comment, every blank line and every other line stay byte for byte.

- (f) Both write the edited file only when it reads back as the file with that one binding changed, and a layout they cannot edit line by line, such as a rule written in flow style, fails the verb with the file unchanged.

With several files on the walk, a person whose stdin and stderr are both at a terminal is asked which, nearest first; a harness, or anyone else, gets the nearest. The README and `docs/cli.md` say only "nearest", and no CHANGELOG entry introduced the question, so it is not a law. The `lade setup` that `lade add` runs after the write can still fail. The edit then stays, `lade add` exits 1 with setup's error after printing what it added, and `lade setup` finishes the job.

## Family names

The README "Families" section: three families, `secret`, `tunnel` and `package`, and family aliases on the CLI only: `env` for `secret`; `net`, `network` and `fwd` for `tunnel`; `pkg`, `cli`, `tool`, `apm` and `skill` for `package`. Scripts and muscle memory type these. `bin` and `mise` are not families since CHANGELOG 0.19.0 (`aliases_map_to_tokens`). The yaml never names a family: a binding's family comes from its URI.

**§ <a id="mwtcwjcr2">mwtcwjcr2</a> On the CLI, a family is named by its token or by one of its documented aliases.**

- (a) `env` names `secret`; `net`, `network` and `fwd` name `tunnel`; `pkg`, `cli`, `tool`, `apm` and `skill` name `package`.
