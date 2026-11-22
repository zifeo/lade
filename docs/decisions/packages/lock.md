# The lock

A pin is only as good as the version it resolves to, and that version must be the same for a teammate, a harness and CI. A second lock beside a repo's own drifts from it, a command that rewrites the lock leaves a diff behind, and a range handed to mise installs nothing. This note binds where the lock lives, who writes it, what it decides, and that every version in it is concrete.

## Where the lock lives

"`lade setup` walks toward `$HOME` once. A committed `mise.toml` or `mise.lock` in the repo is the plane: setup extends both. `$HOME` itself counts only for `~/lade.yaml`. Otherwise one `lade.lock` at the git root. A git repo in `$HOME` is not that root" (`README.md`, Package). A repo that already uses mise has a lock its other tools read; a second lock beside it would hold a second version of the same CLI, so Lade writes into theirs and "does not also write `lade.lock`" (CHANGELOG 0.19.0). A repo without mise gets exactly one lock, at the root, so every subfolder's yaml pins resolve to the same versions (`setup_writes_one_lock_at_git_root`, `root_lade_lock_covers_every_yaml_pin`).

`$HOME` is not a project: people put `$HOME` under git, and a user-wide `~/mise.toml` would otherwise become every repo's lock plane (`a4_home_git_is_not_project_root`, `a10_home_mise_ignored_for_project`). `mise.local.toml` is a personal override, never committed, so it is not a plane (`a9_local_toml_is_not_a_plane`). A mise file above the git root is not "in the repo" (`README.md`, Package): it belongs to another tree, so in a repo the plane walk stops at the git root (`a_mise_file_above_the_git_root_is_not_the_plane`). Outside a repo it runs up to `$HOME`, and from a directory outside `$HOME` up to `/`, so a `/tmp/mise.toml` is the plane of `/tmp/x` (`src/mise/plane.rs`): a stated limit, which no source decides. The sources name `mise.toml` and `mise.lock` (`README.md`, Package) and `.config/mise/config.toml` (CHANGELOG); Lade also reads mise's other config names, `.mise.toml`, `mise/config.toml`, `.mise/config.toml` and `.config/mise.toml`, which no source states. The plane reads only those names; mise's `conf.d` directories and `mise.<env>.toml` files are not read as a plane. A `lade.lock` left after the repo adopted mise is not read: "Lock is the nearest `mise.lock`. A leftover `lade.lock` is unread" (CHANGELOG 0.19.2), and setup says so rather than leave the user guessing which lock counts.

The README calls these files "committed", and a documented promise wins: in a git repo, a `mise.toml` or `mise.lock` git lists as untracked is a file of one machine, so it makes no plane, and setup falls back to `lade.lock` at the git root. Once a tracked file names the plane directory, the other mise file there belongs to the plane even before it is committed, since setup writes it there. Where git cannot answer, with no git binary or no readable repository, a present file counts as before.

**§ <a id="a45aqmr7m">a45aqmr7m</a> `lade setup` keeps a repo's lock in one place: the nearest `mise.toml` or `mise.lock`, which it extends, else one `lade.lock` at the git root.**

- (a) In a repo, the plane is the nearest directory, from the current one up to the git root, that holds a mise config or lock, `mise.toml`, `mise.lock` and `.config/mise/config.toml` among them.

- (b) `$HOME`'s own mise files count only when setup runs in `$HOME`.

- (c) Without a mise plane the lock is one `lade.lock` at a git root other than `$HOME`.

- (d) With a mise plane, Lade locks in that plane alone, and setup warns that each `lade.lock` it finds is ignored.

- (e) When the nearest `mise.toml` and the nearest `mise.lock` sit in different directories, setup writes both in the nearer directory and warns that the other stays as it is.

- (f) In a git repo, a mise file git lists as untracked counts toward the plane only beside a tracked mise file.

## Who writes the lock

The lock and a project `mise.toml` are files a team reviews and commits. A command that rewrote them as a side effect would leave a diff after `make test`, and two teammates' commands would race on it. So a wrapped command reads the lock and installs into mise's store through a config in Lade's own cache: "Installs live in Lade's cache. Shims are not placed on the interactive `PATH`" (CHANGELOG 0.19.0; `inject_does_not_write_mise_files_in_repo`, `inject_leaves_existing_mise_toml_untouched`, `set_then_unset_leaves_repo_without_mise_files`). Only the verbs that exist to change a repo write it. mise still keeps its own records in its own directories, a tracked-config link to Lade's config in its state directory and its download cache, checked against mise 2026.11.10; neither is in the repo.

The project `mise.toml` is the team's file: it holds their other tools, environment tables and comments. Setup edits only the `[tools]` rows of the pins it owns, and a file it cannot parse is one it cannot edit without losing content (`preserves_env_and_comment`, `invalid_toml_is_error_and_file_unchanged`). A CLI the file lacks is added with `mise use --path`, and mise keeps the other tables and comments there, checked against mise 2026.11.10.

**§ <a id="cp6eyqnnj">cp6eyqnnj</a> Only `lade setup` and `lade update` write the lock or a project `mise.toml`, and a wrapped command reads them.**

- (a) A wrapped command keeps its `mise.toml`, its lock link and the shims of its mise runs in Lade's cache.

- (b) Setup changes only the rows of its own pins in a project `mise.toml`, keeps every other table and comment, and fails without writing on a file it cannot parse.

## The lock is the version

The README table is the contract for the four verbs (`README.md`, Package):

| Command | Lock | Yaml |
| --- | --- | --- |
| `lade setup` | Install this version | Exact pins stay. A range may heal from `mise.toml` |
| `lade setup --unlock` | Ignore it this once, then rewrite | Resolve and install what yaml says now |
| `lade update` | Rewrite after a new resolve | Exact pins stay. Implied and ranged pins move to the latest match |
| `lade upgrade` | Untouched | Untouched. This is the Lade binary |

A lock that a newer install on the machine could override is no lock: `setup_locked_installs_lock_not_a_newer_store`. An exact pin in the yaml is the team's explicit choice, so when it differs from the lock the yaml wins and the lock follows (`setup_exact_pin_overwrites_toml`, `inject_lock_mismatch_skips_locked_flag`). "Missing slots are resolved once and written" (`README.md`, Package): the first setup decides, and everyone after it installs the same version. `lade update` moves ranges forward; when mise can list nothing, a locked version still inside the range and newer than Lade's fallback stays (`update_keeps_a_newer_lock_when_latest_prints_nothing`, `update_does_not_downgrade`), and one the range no longer holds is replaced (`update_leaves_a_lock_the_range_no_longer_holds`). The wrap honours the same lock (`parent_lock_triggers_locked_install`, `inject_lock_backend_id_key_uses_locked`).

**§ <a id="byd2nhybz">byd2nhybz</a> The lock is the version a pinned command gets, and only `lade update` or `lade setup --unlock` replace a locked version that still fits the yaml.**

- (a) `lade setup` installs the locked version of each pin whose lock entry fits the yaml, even when a newer one is installed. An exact pin that differs from its entry wins and rewrites the lock.

- (b) A pin with no fitting lock entry is resolved once and the lock is rewritten with that version.

- (c) `lade setup --unlock` ignores the lock this once, resolves from the yaml, and rewrites the lock.

- (d) `lade update` moves implied and ranged pins to the highest version in their range, and leaves exact pins.

- (e) A wrapped command runs the locked version when the lock has a fitting entry.

## Concrete versions only

mise "rejects a range on aqua, github, npm, pipx, and vfox, prints nothing, and exits 0" (CHANGELOG 0.19.1, mise ranges; the 2026.9.15 release). A range handed to `mise install` or `mise lock` therefore installs nothing and writes a range into the lock, with no error. So "a range is not passed to `mise latest`, `mise install`, or `mise lock` ... A range already stored in the project `mise.toml` is left out of the lock request. Setup lists versions with `mise ls-remote` and installs the highest match" (same entry). `aqua:1password/cli` has no listing, so Lade carries one known version for it (2.39.0 today, `src/mise/unlistable.rs`), used only when it fits the range; any other unlisted range is an error before install, not a guess (`unlistable_gap_is_an_error_before_install`, `setup_refuses_a_range_when_nothing_lists_it`). The source names the range case alone: another row of a project `mise.toml` that is not concrete, such as `latest`, goes to `mise lock` as the team wrote it, and mise locks a concrete version for it (checked against mise 2026.11.10, which locked `latest` as `1.8.2` with `specifiers = ["latest"]`).

"`mise://…@latest` is written back as the resolved version" (CHANGELOG 0.19.0), in the yaml that holds the pin, which a wrap finds on the walk as setup does (`a_latest_pin_is_rewritten_in_the_yaml_it_came_from`), and which setup and update write too when the lock already holds an entry, as the version they take (`setup_and_update_rewrite_latest_with_a_lock_entry`): `latest` is a moving target, and a yaml that says `latest` would mean a different CLI tomorrow. `lade add package` writes the resolved version from the start (`README.md`, scheme table: "`lade add package` writes this").

**§ <a id="h01jcjwrv">h01jcjwrv</a> Every version Lade installs or locks is concrete: a range or `latest` is resolved to one listed version first.**

- (a) Lade hands `mise install` and `mise lock` each of its pins at a concrete version, and leaves a range row of a project `mise.toml` out of the lock request.

- (b) A range resolves to the highest listed version inside it, else, for a CLI mise cannot list, to a version this Lade release carries for it when that fits; otherwise setup fails and names the package and the range.

- (c) A `@latest` pin is written back, as the version it resolved to, into the yaml that holds the pin.
