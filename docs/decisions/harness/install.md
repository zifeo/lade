# Installing and refreshing hooks

A harness reads its hooks from config files it shares with the user's other hooks, and a team may commit one as a requirement. A hook written in the wrong place, one that clobbers another hook, or one that goes stale runs commands with the wrong wrap or none. This note binds where Lade writes a hook, what it keeps, how a repo requires Lade, and how the hooks stay current.

## Installing the hooks

"The rest stays in the repo: pre-tool hooks for Cursor, Claude Code, Codex, and OpenCode harnesses" (`README.md`, Getting started). The access is the repo's, so the hook is too: `lade setup` and `lade hook enable --harness <slug>` write it at the git root, and outside git the pre-tool hook is skipped (CHANGELOG 0.20.0, `lade hook enable/disable`). A home directory under git is not a repo (`src/pretool/install/tests/locate.rs`, `project_dest_skips_a_git_init_home`). `--global` writes the harness's home config for this machine instead.

A harness runs every hook it finds, so a repo hook and a home hook for the same harness wrap each command twice. CHANGELOG 0.20.0: "A repo hook and a home hook for the same harness are refused together." A committed Claude hook is a repo hook from the moment it is committed, its local copy written or not, since Claude runs it at the repo root. `lade teardown` removes this repo's hooks and leaves the shell hook and the home hooks (`README.md`, Getting started; `docs/architecture.md` section 7, Installing pre-tool).

A repo hook file is shared with teammates, so it calls `lade` by name; a path to one person's binary breaks it for everyone else. A home hook may pin the installed binary, but a hook that named a cargo build under `target/` ran a test binary in the user's harness (`src/pretool/install/tests/write.rs`, `project_hook_command_is_always_portable`, `hook_command_never_writes_a_test_binary`; CHANGELOG 0.18.0, Hook bin name). A target directory is known by the `.rustc_info.json` cargo writes at its root, so a custom profile or a `CARGO_TARGET_DIR` counts too (`a_build_under_any_cargo_target_dir_is_not_pinned`).

**§ <a id="rkpfgpx72">rkpfgpx72</a> Lade writes a harness's pre-tool hook in one place: the root of this git repo or, with `--global`, the harness's home config.**

- (a) Without `--global`, Lade writes the hook only at the root of a git repo other than the home directory.

- (b) While a harness has a Lade hook in one place, `lade hook enable` for the other fails, `lade setup` flags the home hook, and both present fail `lade setup`.

- (c) `lade hook disable` without `--global` removes only this repo's Lade hooks.

- (d) A repo hook calls `lade` by name. A home hook may name the installed binary, and calls `lade` by name for a binary in a cargo target directory, whatever its name or profile.

What `lade teardown` removes and keeps is [ep7nzy2xg](../packages/setup.md#ep7nzy2xg).

A harness config is the user's file: it holds their permissions, their model, and other tools' hooks. Install merges and keeps `permissions` and `model` (CHANGELOG 0.18.0, `lade hook install` / `uninstall`; `tests/hook_install.rs`, `hook_install_preserves_claude_permissions_and_model`), and removal strips only Lade's entries. A file Lade cannot parse is one it cannot merge into without losing the user's content (`merge_rejects_invalid_json`).

**§ <a id="e3ntx7tnt">e3ntx7tnt</a> Lade changes only its own hook in a harness config, and keeps every other hook and key.**

- (a) Writing adds or updates the Lade hook in place, and removing strips only Lade's hooks and the entries they leave empty.

- (b) A config Lade cannot read, or that is not valid JSON, fails the write and stays as it is.

## Claude Code: a committed requirement

A team that commits the Lade hook in `.claude/settings.json` requires Lade from everyone who clones the repo; with `"onFailure": "block"`, a missing or failing Lade blocks the command instead of running it without Lade (`README.md`, Require Lade on Claude Code). Claude Code reads that file only in a session started at the repo root, and `.claude/settings.local.json` from any subdirectory, so Lade copies the committed hook there. Claude runs a command found in both files once, with the fields of the local copy, so the copy is verbatim: a copy without `onFailure` would turn the block off (CHANGELOG 0.20.1, Required Lade on Claude Code).

0.20.0 moved the committed hook to the local file and stripped it, which silently dropped the team's requirement (CHANGELOG 0.20.1, Upgrading). The committed hook is the decision, so Lade only reads it: enable, setup, the refresh, disable and teardown leave it, and disable says so. From a linked git worktree Claude reads the local file at the main checkout's root, so a copy at the worktree root never ran (CHANGELOG 0.20.2, Claude Code in a git worktree). The refresh writes the copy with no `lade hook enable`, since the committed hook already asked for it; a home hook for Claude Code would run too, so then nothing is written and status flags both (CHANGELOG 0.20.2, The committed Claude hook applies by itself).

**§ <a id="dpgrarwms">dpgrarwms</a> A Lade hook in a repo's committed `.claude/settings.json` is the repo's requirement, and Lade copies it where Claude Code reads it from any directory.**

- (a) Lade reads the committed file as the requirement, and writes its Claude Code hooks only to `.claude/settings.local.json` or Claude Code's home config.

- (b) The repo plane writes a verbatim copy of the committed Lade hooks, `onFailure` included, into `.claude/settings.local.json`, in place of Lade's own hook.

- (c) From a linked git worktree the copy goes to the main checkout's root, and a Lade hook in the worktree's own `settings.local.json` is removed.

- (d) The refresh writes the copy without a prior `lade hook enable`, unless Claude Code has a Lade hook in its home config.

- (e) `lade status` reports the committed hook, blocking only when every committed Lade hook has `"onFailure": "block"`, and flags a local copy that is missing or differs.

## Cursor, OpenCode and Codex: the repo hook

The other harnesses have no committed file beside the one Lade writes: Lade's repo hook for Cursor, Codex and OpenCode is the repo's file (`.cursor/hooks.json`, `.codex/hooks.json`, `.opencode/plugins/lade-pretool.js`), so that hook is where a repo requires Lade (owner answer, 2026-11-19). Each harness decides what a failed hook does. Cursor fails open, and a hook with `failClosed` blocks: "When `true`, hook failures (crash, timeout, non-zero exit code, no output) block the action instead of allowing it through" (Cursor hooks, per-script options, https://cursor.com/docs/agent/hooks). The page states it for every hook definition and names `beforeShellExecution`, `beforeMCPExecution` and `beforeReadFile`; Lade's hook is `preToolUse` with the `Shell` matcher, and the page gives that event no other failure rule. OpenCode blocks a tool call whose `tool.execute.before` throws (OpenCode plugins, https://opencode.ai/v1/docs/plugins/), and its plugin is Lade's own code. Codex runs the tool call after a failed command hook and documents no per-hook failure policy, only exit 2 or a deny answer (Codex hooks, https://developers.openai.com/codex/hooks): on Codex the hook rewrites lines and Codex decides failures.

A home hook is one machine's, not a repo's. A home hook that blocked every shell call once Lade is gone would lock that machine's sessions until the hook is found and removed, so the home hook leaves a failure to the harness, as before.

**§ <a id="ck03z9rny">ck03z9rny</a> A repo's Lade hook on Cursor and OpenCode blocks the tool call when Lade cannot start or fails, and a home hook leaves that failure to the harness.**

- (a) Lade writes the repo's Cursor hook with `"failClosed": true` and the home hook without it, keeps a `failClosed` the user set on a home hook, and a refresh writes it into a repo hook that lacks it.

The OpenCode half is the plugin's, and the plugin carries no marker (see Answering the harness). A plugin whose file sits in a `.opencode/plugins/` directory, where OpenCode loads a repo's plugins, throws when it cannot start Lade, Lade exits non-zero, or Lade's answer is not JSON; the home plugin, under `~/.config/opencode/plugins/`, writes the same line on stderr and runs the line unchanged (`src/pretool/install/tests/opencode.rs`, `a_repo_plugin_blocks_the_call_when_lade_cannot_run`, `a_home_plugin_runs_the_line_when_lade_cannot_run`). The plugin reads its place from its own path, so one text serves both planes and refresh compares it as before.

## Keeping hooks current

A hook Lade wrote can go stale: a newer Lade writes another command, or the repo changed its committed hook. A stale hook is a harness running commands with an old wrap, or none. So Lade rewrites what it wrote, at moments a human is around: the hourly notice slot of a shell command that uses the hub, and every time a shell starts a harness (`opencode`, `claude`, `codex`, `cursor-agent`, `agent`), without the hub (`AGENTS.md`, House rules, Shell notices; CHANGELOG 0.20.0, Release check and notices). It never creates a hook the user did not install (CHANGELOG 0.18.0, Daily hook refresh), the committed Claude copy aside.

What Lade cannot fix, it says. A hook file that is not valid JSON is the user's to fix: rewriting it would lose their content. An OpenCode plugin edited by hand is the user's code. Both are listed in the notice box and in `lade status` (`src/pretool/install/tests/health.rs`, `invalid_json_is_reported_without_a_rewrite`, `refresh_leaves_a_hand_edited_plugin_and_reports_it`). `lade status --json` keeps its `hooks` object, `preexec` plus `pretool` (`AGENTS.md`, House rules), with each harness's home and repo hook.

**§ <a id="khcwz2ec3">khcwz2ec3</a> Lade keeps the hooks it wrote current without being asked, and tells the user about every hook it cannot fix.**

- (a) The refresh rewrites a Lade hook already in a file to what this Lade writes, and creates only the copy of a committed Claude hook.

- (b) A hook file that is not valid JSON, or an OpenCode plugin edited by hand, is reported and left as it is.

- (c) A shell command that starts a harness checks and refreshes that harness's hooks every time, without the hub. The hourly notice slot covers every harness.

- (d) `lade status` lists the same notices, and its JSON reports each harness's home and repo hook. A hook file Lade cannot read is reported as unreadable.

OpenCode 1.x calls every function a plugin file exports, and 2.x wants a default export with `id` and `setup`: one file cannot serve both (`src/pretool/install/opencode.rs`). Lade picks the API from `opencode --version` when it writes the plugin, and fails when `opencode` is not on `PATH` rather than guess. A repo plugin is shared by teammates who may run different OpenCode versions, so a refresh keeps the API of the file instead of flipping it per machine (CHANGELOG 0.20.0, OpenCode plugin). Only the automatic refresh keeps the file's API. `lade setup` and `lade hook enable` are the user asking for a plugin on this machine, so they write it for the `opencode` on `PATH`, even over a plugin of the other API (test `opencode_plugin_follows_the_installed_opencode`).

**§ <a id="heg7hfveq">heg7hfveq</a> Lade writes the OpenCode plugin for the API of the `opencode` on `PATH`, and a refresh keeps the API of the file.**

- (a) `lade setup` and `lade hook enable` write the plugin for the API of the `opencode` on `PATH`, even over a plugin of the other API, and fail when they cannot tell that API.

- (b) A refresh keeps the API of the plugin file, and a plugin whose API differs from the installed `opencode` is reported.

CHANGELOG 0.20.0 names a 1.x and a 2.x plugin. Lade gives every major from 2 on the 2.x API, and cannot tell the API when `opencode` is not on `PATH` or prints a version it cannot read (`src/pretool/install/opencode.rs`); no source gives those cut-offs.
