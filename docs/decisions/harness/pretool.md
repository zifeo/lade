# The pre-tool hook

The harness owns the reply format, the working directory and the shell its command runs in, and the pre-tool hook must fit each: a rewrite in another harness's envelope is ignored, a hook that reads its own directory loads the wrong yaml, and a line run in the wrong shell breaks. This note binds what the hook answers, what it rewrites, which directory it reads and which shell the wrap runs in.

## Answering the harness

Each harness reads its own reply: Claude Code and Codex a `hookSpecificOutput` with `updatedInput`, Cursor `permission` with `updated_input`, the OpenCode plugin `{ command }` (`src/pretool/mod.rs` header). A harness that gets another's envelope ignores the rewrite and runs the original line, which is a command without its access and without a word. That happened: `CURSOR_VERSION` leaks into other harnesses' terminals and made Lade answer Claude Code in Cursor's format (CHANGELOG 0.18.0, preTool envelope). Since then every installed hook names its harness (CHANGELOG 0.18.0, `lade hook --harness`), and detection without the flag reads the payload before the environment, `CURSOR_VERSION` last (`docs/architecture.md` section 7, pre-tool path).

A tool input holds more than the command: Cursor sends `working_directory`, Claude Code a `description` or a `timeout`. The rewrite sends the input back with only the command replaced, so the harness runs the call it asked for.

**§ <a id="j0my6hfhx">j0my6hfhx</a> The pre-tool hook answers each harness in that harness's own hook format.**

- (a) Every hook Lade installs passes `--harness <slug>`, and the hook answers in the format of the harness it names.

- (b) Without a known `--harness`, the payload's event names the harness before any environment variable does, and `CURSOR_VERSION` is read last.

- (c) A rewrite replaces only the command: Cursor, Claude Code and Codex get their tool input back with its other fields, and OpenCode gets `{ command }`.

- (d) A refusal is the harness's deny with Lade's reason: `permissionDecision: deny` for Claude Code and Codex, `permission: deny` for Cursor, and for OpenCode, whose plugin takes only a command, a rewrite into the wrap, which refuses the line.

The deny answers are the ones each harness documents for a pre-tool hook (Claude Code and Codex hooks, `permissionDecisionReason`; Cursor hooks, `user_message` and `agent_message`), and each blocks the tool call whatever the hook's failure policy. The OpenCode plugin runs whatever command Lade answers, so the wrap is its deny: it loads the same walk, fails, and exits 1 without running the line.

The OpenCode plugin is the one host copy Lade ships as code: `src/pretool/install/opencode_v1.js` for OpenCode 1.x and `.opencode/plugins/lade-pretool.js` for 2.x. Both send `{ command, session_id }` for a shell tool call to `lade hook --harness opencode`, run the `command` Lade answers, pass Lade's stderr on, since OpenCode shows none of it, and say on stderr when they cannot start `lade` (CHANGELOG, Unreleased, The OpenCode plugin: "instead of dropping both"), or block the call when they are a repo's plugin, [ck03z9rny](install.md#ck03z9rny). They run the `lade` a set `LADE_BIN` names, else the one they were written for, as `docs/env.md` says and as the age plugin does, [mcf6rtczf.b](../secrets/eval.md#mcf6rtczf). These are promises with no law of their own: they are written verbatim into users' repos and compared byte for byte on refresh, against this Lade's text and every text an earlier release wrote (`src/pretool/install/opencode.rs`), so they carry no marker: tests guard them.

A hook that cannot read its payload never saw a command, so nothing matched and there is nothing to withhold. Blocking the tool call there would stop a harness after an update to its payload format, for every command, matched or not. CHANGELOG 0.18.0 (Unreadable `lade hook` payload): invalid JSON or a pre-tool event with no command still allows the tool call, and stderr now says so, "so a later vault or lock error is not the first clue". The same release: "Unknown values are ignored. Detect and ticket writes fail open so a harness change does not block the tool call" (`lade hook --harness`). A `lade.yaml` that does not load keeps this rule: a payload with no command still lets the tool call run, while a line with a command is denied, [y4zs3xgg4](../config/discovery.md#y4zs3xgg4).

**§ <a id="f7eb9ymg0">f7eb9ymg0</a> A pre-tool hook that cannot read a command from its payload lets the tool call run unchanged, and says so on stderr.**

- (a) Once the hook's rules load, a payload that is not JSON, bytes that are not UTF-8 included, or a pre-tool event that holds no command, is allowed with a warning.

- (b) The hook ignores a `--harness` value Lade does not know, and goes on as for a known harness when it cannot detect one.

## What the hook rewrites

The hook resolves nothing, [g6sqmp8tb](../promises.md#g6sqmp8tb): it decides whether the line goes through the wrap. A wrap costs a process and a ticket, so a command that has nothing to prepare runs as sent (`src/pretool/tests/wrap.rs`, `test_log_only_dot_does_not_wrap`, `test_pin_only_is_rewritten`, `test_disclaimer_only_is_rewritten`; `src/pretool/tests/when.rs`, `user_keyed_secret_skips_wrap_when_os_user_misses`). No match is silent (CHANGELOG 0.18.0). Which secrets a rule binds may depend on the Lade user, so a `config.json` the hook cannot read leaves it unable to plan a rule with any binding, and that line goes to the wrap, which fails it. Matched rules that bind nothing prepare the same for every user, so the hook decides without the user: a disclaimer still goes to the wrap, and a `log`-only `.` runs unchanged, only its row, which needs the user, lost with a warning, as for an unmatched line (`src/pretool/tests/when.rs`, `a_log_only_match_with_a_broken_config_json_runs_unchanged`).

A harness may itself type `lade -- tofu apply` or `lade inject tofu apply`. Matching that line as is would miss `^tofu`, and wrapping it would nest Lade in Lade. The hook peels Lade's own vehicles to the inner command, never emits `lade inject`, and leaves a clap verb such as `lade status` alone, since it is not a vehicle (`docs/protocol.md`, Paths; CHANGELOG 0.20.0, Pre-tool rewrite). A line that is already a finished wrap, whose ticket holds that very command, is left as it is.

A leading `LADE_APPROVE=<code>` is how a human's approval reaches the wrap (`docs/architecture.md` section 7). Moved in front of the rewrite, it lands in the wrap's environment. A value with quotes or expansion may hold spaces, and splitting it broke the line (CHANGELOG 0.20.0, Assignments in a pre-tool line), so only a plain value moves. The hook also leaves `LADE_T`, `LADE_RESTORE` and `LADE_VIA` out of the moved words; no source makes that a promise, and the wrap strips them from the child anyway (`docs/protocol.md`: "The child never sees the id or `LADE_T`").

A bare `cd`, wrapped, moves Lade's child and leaves the harness shell where it was: the next command runs in the wrong directory (CHANGELOG 0.20.0, Pre-tool hooks in a subdirectory; `docs/protocol.md`, Working directory). That holds when the hook cannot plan the line's rules too, a `config.json` it cannot read for one: the move runs, and the hook says it kept no diary row (`tests/hook.rs`, `a_bare_cd_the_hook_cannot_plan_runs_as_sent`). A `lade.yaml` that does not load denies a bare `cd` too, as any line that does not start Lade, [y4zs3xgg4](../config/discovery.md#y4zs3xgg4): the hook cannot tell which rules it would have recorded. Anything compound is a normal line.

An MCP tool call reaches the hook as a verb, and a post-tool event after the fact: neither is a shell line to wrap. "MCP verbs are allow-only" (`docs/architecture.md` section 7); the hook only records a `seen` row when `log` asks for it (CHANGELOG 0.18.0, MCP verbs and diary argv). A verb whose rules the hook cannot plan is allowed too, with a warning that no row was kept: a hook that exited 1 there was blocked under a committed `"onFailure": "block"` (`tests/hook.rs`, `an_mcp_verb_the_hook_cannot_plan_is_allowed_with_one_lade_line`).

**§ <a id="hk9h1sqyt">hk9h1sqyt</a> The pre-tool hook wraps a harness's command only when the rules that match it give it access to prepare.**

- (a) Access to prepare is a secret, a tunnel, a pinned CLI or a disclaimer. A command with none of them runs unchanged, with a warning only when the hook cannot write the diary row its rules ask for, unless a rule binds it a setup package, which goes to the wrap to be refused.

- (b) The hook matches the command Lade would run: it peels `lade inject`, `lade --` and `lade <cmd>` to the inner command, which runs bare when nothing wraps it. It peels those vehicles alone: a Lade verb is matched as written, and a finished wrap runs as written.

- (c) A plain `LADE_APPROVE=<code>` in front of the command moves in front of the rewrite, so it reaches the wrap. Another leading `NAME=value` moves there only when its value is plain and the command follows it.

- (d) Once the rules of its directory load, a line that is only `cd`, `pushd` or `popd` with at most one literal argument runs unchanged, whatever they bind.

- (e) A hook call for an MCP tool, or after a tool ran, allows it unchanged, and says so on stderr when it cannot plan the tool's rules.

## Where the command runs

The hook process starts wherever the harness starts it, often the repo root, while the command runs in a subdirectory with its own `lade.yml`. Before 0.20.0 the hook read its own directory, so a subfolder's rules never loaded (CHANGELOG 0.20.0, Pre-tool hooks in a subdirectory). The payload says where the harness runs the command: `cwd` from Claude Code, Codex and Cursor, then Cursor's `tool_input.working_directory` or Codex's `tool_input.workdir`, relative to `cwd`; the OpenCode plugin sends neither (`docs/protocol.md`, Working directory). `^tofu` must also match `cd infra && tofu apply`, so leading `cd <dir> &&` segments move the directory and leave the line the rules see.

Lade reads those segments without running a shell, so it follows only what it can resolve for certain. A `$VAR`, quotes, `cd -`, a missing directory, or a bare name `CDPATH` would search stop the move at the last directory resolved: guessing would load another folder's secrets (`src/pretool/tests/cwd.rs`, `unresolvable_cd_keeps_the_last_resolved_directory`, `cdpath_leaves_a_searched_cd_to_the_shell`). The line still runs whole, `cd` included, so the shell moves as the harness meant. The wrap resolves the same segments from its own directory, [g6sqmp8tb.d](../promises.md#g6sqmp8tb).

**§ <a id="v82zzh1yh">v82zzh1yh</a> The pre-tool hook reads the rules of the directory the harness runs the command in.**

- (a) That directory is the payload's `cwd`, then Cursor's `working_directory` or Codex's `workdir` resolved against it. A payload with neither leaves the hook's own directory.

- (b) Each leading `cd <dir> &&` moves it, only to an existing directory written as a literal path, `~` or `~/…`. The first segment it cannot resolve so stops the move, and the rules see the line after the applied segments.

## The harness's shell

A harness writes a command for its own shell. A harness started from a fish terminal with `lade on` inherited `LADE_SHELL`, and Lade ran every wrapped bash or zsh line in fish, which broke loops and redirections (CHANGELOG 0.20.0, Pre-tool commands in the wrong shell). Since then the wrap runs its line in the shell that started Lade, its parent, never one an inherited variable names, and a pre-tool wrap with no shell parent fails, [dpky46ff1.d](../wrap/child.md#dpky46ff1) (`README.md`, Shell and harnesses: "A wrapped command runs in the harness's own shell, even when the harness started in a terminal with `lade on` in another shell").

That only works if the harness shell is still Lade's parent when the wrap starts, and the rewrite is what keeps it there. bash, zsh and sh exec the last command of a `-c` line in place of themselves, and Codex runs `zsh -lc '<rewrite>'`, so a rewrite that ended with Lade would make Codex Lade's parent. A command after Lade keeps the shell, and `&& true` keeps Lade's exit status, fish included (`src/pretool/classify.rs`; `tests/pretool_shell.rs`, `hook_rewrite_keeps_the_harness_shell_as_parent`).

**§ <a id="bwgf7z8q5">bwgf7z8q5</a> A pre-tool rewrite keeps the harness's own shell as the parent of the wrap it starts.**

- (a) Every rewrite that starts Lade ends with `&& true`, so a shell that execs the last command of a `-c` line stays Lade's parent, and Lade's exit status is kept.

The wrap a rewrite starts names a binary too. CHANGELOG 0.18.0 (Hook bin name) says a path invocation uses `current_exe`, and the code named bare `lade` whenever a `lade` was on `PATH`, since a harness such as Cursor often execs the hook by its resolved absolute path, and that absolute path otherwise. The owner settled it (2026-11-19): the rewrite names bare `lade`, the name a repo hook and the shell profile line use, [rkpfgpx72.d](install.md#rkpfgpx72) and [aq4rxhrqa.a](../wrap/preexec.md#aq4rxhrqa), and the harness's shell resolves it on its `PATH`. A home hook that names an installed binary can then hand its ticket to another `lade` that comes first on that `PATH`, another version with another hub: which `lade` comes first is the user's setup. A harness shell with no `lade` on its `PATH` fails the rewritten line with its own `command not found`, so the line does not run.

**§ <a id="yjys1hgbd">yjys1hgbd</a> A pre-tool rewrite starts Lade by the bare name `lade`, which the harness's shell resolves on its `PATH`.**
