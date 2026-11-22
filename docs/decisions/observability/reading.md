# Reading the diary

A query run in one repo that mixes in every other repo's commands answers the wrong question, and a pack sent to a colleague must carry neither the sender's user name nor the paths of their machine. This note binds what a query reads, what a shared pack holds, how packs are read, and what `lade usage` counts.

## What a query reads

"Queries stay on the current git root (worktrees count). `--all` / `--global` read every repo. `--path` scopes to another tree" (`docs/observability.md`, Read; `README.md`, Observability: "Queries stay on this git root. `--all` / `--global` reads wider"). A row carries the git root of the directory whose rules it read, so `lade usage` in one repo counts that repo's rules only (`log_and_usage_scope_to_git_root`). A linked worktree has its own root (`git_stamp_treats_gitfile_as_worktree_root`), so two branches checked out side by side do not mix. `lade log prune --keep` and `lade log share` take the same scope: a prune in one repo deletes nothing of another.

Outside any git root there is no root to stay on. Reading every repo there would turn a plain `lade log prune --keep 1d` in the home directory into a prune of every repo; a query there fails and names `--all`, as `--path` to a directory outside a repo already does. The default 90-day window and the `--limit` cap are defaults with no stated reason, not laws.

**§ <a id="ck86fwnt7">ck86fwnt7</a> A diary query reads only the rows of the current git root, unless `--all`, `--global` or `--path` names another scope.**

- (a) A row's `repo` is the git root of the directory whose rules it read, a linked worktree being its own root, and `git_commit` that root's checked-out commit.

- (b) `lade log`, `lade usage`, `lade log share` and `lade log prune` read this git root's rows, every repo's with `--all` or `--global`, and with `--path` the rows of that directory's git root.

- (c) Outside a git root, without `--all` or `--global`, a query fails with exit 1 and names `--all`.

## Sharing a slice of the diary

"`lade log share`: Gzipped snapshot (`lade-$FROM-$TO.tar.gz`) with its own chain" and "reseals the redacted snapshot. That pack verifies on its own" (`docs/observability.md`, Read and Store). The receiver checks the pack without the sender's diary, and the sender's diary is never in the pack. A pack holds no absolute path of the sender's machine and no user name (owner answer, 2026-11-19; `docs/observability.md`, Share): it names the sender's files relative to the repo or home they ran in, and its name carries only the time range (`src/event/pack/tests.rs`, `a_pack_names_every_path_relative_to_the_repo_or_home`, `redact_strips_home_and_git_root_prefixes`; `tests/log_share.rs`, `a_pack_holds_no_absolute_path_of_the_sender`, `share_writes_pack_in_cwd`, `share_redacts_actor_and_repo_basename`). A row whose command was dropped by the scrub carries nothing to share, except an MCP row, which is about the server (`share_drops_empty_command_rows`, `share_keeps_empty_mcp_command`). Share never overwrites a file (`share_does_not_overwrite`), and a harness session cannot share (`share_refuses_agent_audience`): a pack is how diary rows leave the machine, and the tests fix that a person, not an agent, does that.

The repo is the nearer place, so a path under a repo inside the home is named from the repo. A directory counts only at a boundary, so `/Users/samantha/bin/x` under home `/Users/sam` is a path outside both and keeps `x` (`redact_cuts_a_prefix_only_at_a_directory_boundary`). The fields are free text, so a path is found by its form: a `/` that opens a value or follows a space, a quote, `=`, `:`, another separator or a short flag such as `-I`, up to the next separator, and the repo or home anywhere in a value. A URL keeps its host, which names another machine. A `/` inside any other word, such as `lib/opt/x`, reads as a relative path and stays, and so does a `file://` URI that names a host, `file://host/p`. A pack's `repo` keeps the repo's directory name, since a scoped read of packs matches by it ([c370d77w5.c](reading.md#c370d77w5)).

**§ <a id="abp3kwm4w">abp3kwm4w</a> `lade log share` writes one pack of the diary rows in its scope and window, redacted, with a chain of its own that verifies without the diary it came from.**

- (a) The pack is a gzipped tar of `manifest.json` and `events.db`, written as `lade-<from>-<to>.tar.gz` in the working directory unless `--output` names a path, and share fails when that path already exists.

- (b) A packed row drops its `actor`, keeps its `repo` as the root's last path component, and in every other field writes each absolute path relative to that root, else as `~/` and the rest under the home directory, else as its last component.

- (c) A row with no command is left out, unless it is an MCP row.

- (d) The pack's rows are sealed oldest first into a new chain, and its manifest names that chain's head and its row count.

- (e) In a harness session, share exits 1 and writes nothing.

## Reading packs

"`--source`: Read packs (or `local`) without writing the live db" (`docs/observability.md`, Read). A reviewer who receives packs from several teammates reads them together, with or without their own diary, and the same pack given twice counts once (`tests/log_source.rs`, `source_local_and_pack_unions_without_duplicates`, `source_same_pack_twice_is_noop_count`, `source_directory_reads_all_packs`, `source_directory_is_not_recursive`). Share and prune change or copy the live diary, so they refuse `--source` (`share_and_prune_reject_source`). SQLite opens each source read-only, the live diary included, and the read runs with `query_only` set (`a_source_is_attached_read_only`). A pack's `repo` is the sharer's root name only, so a scoped read matches it by name as well as by path (`share_redacts_actor_and_repo_basename`). The name is all a pack keeps, so a scoped read also takes the pack rows of any other repo with the same last path component.

**§ <a id="c370d77w5">c370d77w5</a> `--source` reads packs, or the live diary as `local`, read-only.**

- (a) A source is a pack, a directory whose `.tar.gz` packs are read without descending into subdirectories, or `local`, and rows from every source are read together, a row found in several counting once.

- (b) A read with `--source` opens its sources read-only, and `lade log share` and `lade log prune` refuse `--source`.

- (c) Scoped to a git root, a pack's row matches by that root's path or by its last path component.

## Which rules fire

"`lade usage` lists rules that fired. Unused rules are omitted" (`docs/observability.md`, top), "most frequent first" (Read table), and it "omits unused rules and the catch-all rules `.` and `.*`" (Read). A row names a rule only when that rule owns a public binding that won for the command, so usage counts the rules that gave a recorded command something. A rule that matched without a winning binding never shows: one that only sets `log` or a disclaimer, one whose keys a later rule replaced, or one whose bindings are all private `.NAME`. A rule absent from usage may still have matched. A catch-all matches every command and would top every list without saying anything. `.*` matches the same lines as `.` (`src/catalog.rs`, `is_catch_all`).

**§ <a id="jaz8q7kqe">jaz8q7kqe</a> `lade usage` counts, for each rule, the diary rows in scope whose matches name it, and lists only the rules some row names.**

- (a) Each listed rule is one yaml directory and rule, counted once per row that names it, most frequent first.

- (b) Usage lists every rule some row names except a catch-all rule, one that matches every command, such as `.`.
