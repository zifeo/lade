# Where a resolved value shows

A resolved value reaches the command, and from there it can travel further: into a terminal, a CI log or an agent's context through the command's output, onto disk through a `.file` output, into the diary through a command line that holds it. This note binds what masking hides, how long a `.file` output lives, and what a command line loses before the diary stores it.

## Masking

The command's output reaches a terminal, a CI log, or an agent's context. A value a provider resolved is masked there, so it does not leak into what is read, logged or sent on (`README.md`, Secret; `docs/architecture.md` section 3). Masking needs Lade to sit between the command and its output, which only a wrap does: `lade -- cmd`, `lade inject`, and the pre-tool hooks of harnesses (`README.md`, Secret: "In a wrap (`lade -- cmd`, pre-tool)"). In pre-exec the command runs in the user's own shell and its output is not masked. `lade mcp` relays a stdio server's stdout as a JSON-RPC channel, unmasked, as [aeffh6p49.a](../harness/mcp.md#aeffh6p49) binds. A raw value is not masked: it is already in the yaml, and masking short literals such as API versions broke unrelated output (CHANGELOG, #149). A value built from a masked one, such as `Bearer ${TOKEN}`, would hand the token out if it were not masked too. `--no-mask` exists so a human, a recording or a test can see that the command got the real value.

**§ <a id="qq076qjq2">qq076qjq2</a> In `lade --` and the pre-tool wrap, every value a provider resolved is masked in the command's output, unless `--no-mask`.**

- (a) A binding is masked when a provider other than raw resolved it, or when it is built from a masked binding.

- (b) Masking covers every masked binding the command receives: in its environment, in a `.file` output, or only inside another binding through a private binding.

- (c) `--no-mask` turns masking off for that wrap.

The default mask is a shell expansion, `${NAME:-REDACTED}`, so a masked line pasted back into a shell expands to the real value and still runs (CHANGELOG, #135). Output arrives in reads of any size, and a secret cut across two reads is still a secret. Holding back the tail of a read is what lets masking see it whole, but holding more would freeze a prompt such as `Enter a value: ` that has no newline. On Linux the terminal answers `EIO` once the command exits, and treating it as an error dropped the held tail (CHANGELOG 0.20.0, "Lost end of output on Linux").

**§ <a id="bjcpdnsd2">bjcpdnsd2</a> Masking replaces each masked value in the output by its binding's name, however the output is split.**

- (a) Masking finds a masked value anywhere in the output. When one value starts another, the longer one is masked.

- (b) Each match becomes the mask format with `{}` replaced by the binding's name. The format is `${NAME:-REDACTED}` unless `--mask-format` sets another.

- (c) When several bindings share one value, the mask shows one of their names, picked in a fixed order.

- (d) A value split across two reads is still masked. Masking holds back no more output than the longest masked value needs, and writes what it holds when the command ends.

- (e) Masking passes every output byte outside a masked value through unchanged, whatever its encoding.

Duplicate values are handled (CHANGELOG, #138), so one run shows one name for a shared value; the order is the names sorted, first kept (`src/redact.rs`, test `test_duplicate_value_picks_sorted_first_name`), a choice no source gives a reason for.

A command's output is bytes, not text: 0.12.1 fixed a panic "when subprocess lines are not valid UTF-8" (CHANGELOG 0.12.1, #110), and a wrap stays transparent for what it does not mask as for the exit code, [tk1e8j9nf.b](../promises.md#tk1e8j9nf). Masking matches and copies bytes and never decodes them (`src/redact.rs`; test `bytes_outside_a_masked_value_pass_unchanged_in_any_encoding`).

## Secrets as files

Some commands read credentials from a file, not the environment. `.file` on a rule writes that rule's bindings to a JSON or YAML file for the command, then deletes it (`README.md`, Secret). That is output, not the `file://` provider. The file is deleted after the command, so a file Lade overwrote would be a user's file Lade then deletes (`src/files/tests.rs`, `test_write_files_already_exists_error`).

**§ <a id="fz8r6d2ka">fz8r6d2ka</a> `.file` on a rule writes its bindings to a file for the command, then deletes it.**

- (a) The extension sets the format: `.json` is JSON, `.yaml` and `.yml` are YAML. Any other extension, or none, fails the command.

- (b) Lade creates the file new, and a path that already exists fails the command.

- (c) A write that fails deletes every file it created.

- (d) Lade deletes the file when the command ends, and when it cancels the command.

- (e) In pre-exec, the `lade unset` after a command deletes exactly the files Lade wrote for it, which its ticket records.

The file is written only once every secret and tunnel of the command is ready, so a Ctrl-C while tunnels start has nothing to delete.

Under (b) the path may hold the user's own file, which made the command fail. Run anyway, such a command gets no ticket, so the `lade unset` after it deletes nothing (test `a_file_of_the_user_survives_a_run_anyway_and_the_unset_after_it`); it once deleted every `.file` path the line's rules name. `lade mcp` deletes the file when a client stops it with `SIGTERM` or `SIGINT`, and ignores a hangup as every wrap does (test `a_stop_signal_to_an_mcp_bridge_removes_its_file_output`).

Two limits stay. In pre-exec the delete is the after-hook's `lade unset`: a shell killed while its command runs leaves the file, and so does a ticket removed from the temporary directory before that unset runs. Lade dies on `SIGKILL` with nothing deleted.

## The diary

The diary records which access ran, never a value (`docs/observability.md`). A command line can hold a secret: the user may paste a token into it, or a value Lade resolved may come back in it. The line is scrubbed before it is stored: a known value gives way to its name, and what still looks like a credential gives way to `?` (`docs/observability.md`, Shape).

**§ <a id="ec0ncyt0n">ec0ncyt0n</a> A command line reaches the diary only after the secrets in it are taken out.**

- (a) Each value Lade resolved for the command becomes `${NAME}`. A line, or an argument list, that still holds one after that is dropped from the row.

- (b) The leading `NAME=value` assignments are dropped whole, a quoted value with blanks in it included.

- (c) A token that looks like a credential becomes `?`: the token after an auth header or a key parameter, a token with a known key prefix, or a long token that mixes letters and digits.
