# Changelog

All notable changes to [Lade](https://github.com/zifeo/lade) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Release notes are also published on [GitHub Releases](https://github.com/zifeo/lade/releases).

## [Unreleased]

### Changed

- **User-facing copy**: crates.io / README / `lade --help` lead with
  temporary access for one command, then gone, same wrap for humans and
  agents, plus which access was used. Spoken terms are pre-exec /
  pre-tool. Empty parse Hints dropped. README keeps one install tell;
  last-wins / `seen` stay in `docs/observability.md`.
- **Observability**: `docs/log.md` is `docs/observability.md`. README
  section matches. `lade log` / `lade usage` verbs stay.
- **Version bump**: `scripts/set-version.sh` runs `cargo set-version`
  and rewrites `apm.yml`. Release CI uses it. Do not write `apm.yml`
  from `build.rs`.

### Fixed

- **Diary writers**: if `to_latest` fails but `pending_migrations`
  is 0, the peer finished the schema and the loser still inserts.
  Busy wait stays 250ms.

### Added

- **APM package**: `apm.yml` plus `.apm/skills/lade` (link to
  `.agents/skills/lade/SKILL.md`). Consumers pin the GitHub tag
  `zifeo/lade#vX.Y.Z`. `apm.yml` `version` tracks the crate (release bump
  rewrites it). Hooks stay native.
- **`lade hook install` / `uninstall`**: `--scope user|project` and
  `--harness claude|cursor|codex|opencode`. The binary serves the repo
  snapshots. Merge keeps `permissions` / `model`. Codex user scope honors
  `CODEX_HOME`. `lade status` labels user and project (JSON `global` is
  the user plane) and prints `run \`lade install\`` on drift.
- **`lade install --cursor|--claude|--codex|--opencode`**: select
  harnesses. A git cwd defaults to this repo (hooks and skills). No
  git defaults to this machine. Confirm detected agents (default all).
  A complete default plane skips prompts. Installing the repo plane
  warns when machine hooks already exist (both processes still run).
  A stale Lade-managed file is rewritten. Unmanaged skills stay.
- **Unreadable `lade hook` payload**: invalid JSON or a preTool event
  with no command still allows the tool call (empty Claude/Codex stdout).
  stderr now says so, so a later vault or lock error is not the first
  clue. No-match remains silent.
- **Local command diary**: `lade log` and `lade usage` read a local SQLite
  WAL at `ProjectDirs` `data_local_dir()/events.db` (same library and
  qualifier as `config.json` on `config_local_dir()`). Recording is opt-in
  with `log: true` on matching `lade.yml` rules. Secret values are never
  stored. Default read window is 90 days. `--since` / `--until` bound
  it. `--limit` is an extra cap. `lade log --group command` counts
  commands. `lade usage` lists matched rules in this tree, most
  frequent first, with the file path.   `lade.yml` walk stops at
  `$HOME`. Queries stay on the current git root, including
  worktrees. `--all` reads every repo. `--path` scopes to another
  tree.
  `lade log prune --keep` is the only delete. `lade status` reports
  event count and a human size. `--json` keeps raw `bytes` and does
  not change `ok`.
- **`.when` audience detection** ([#184](https://github.com/zifeo/lade/pull/184)): `detect()` classifies every invocation as
  preexec, pretool, or unset. `--pretool` (global) wins, then the
  subcommand, then env signals when Via is empty. Leftover `LADE_VIA` is
  ignored. MCP and a bare `lade inject` use the same function. The same
  pattern can be a list of bodies with different `when`.
- **`.silence` hydration progress** ([#184](https://github.com/zifeo/lade/pull/184)): `silence: true` under `.` skips that
  rule's secret progress lines at hydrate time. Hydration itself is unchanged.
- **`lade bench`** ([#190](https://github.com/zifeo/lade/pull/190)): times the incompressible path (parse all `lade.yml` files
  and regex match) and the variable path (secret hydrate per loaded rule).
  Human mode prints parse/match first, then each rule as it finishes, then
  wall-clock `total`. Hydrates run concurrently. `--timeout` caps each rule
  (`5s` by default). Errors sit on the next indented line. `--json` includes
  `total_ms` and `timeout_ms`. Secret values are not printed. Network acquire
  is not run.
- **Claude-compatible preTool hosts** ([#191](https://github.com/zifeo/lade/pull/191)): `lade hook` keeps an explicit detect
  path for Codex, Pi, and OpenCode, then rewrites with the same `updatedInput`
  envelope as Claude Code. `lade install` / `status` cover
  `~/.codex/hooks.json`, `~/.pi/agent/settings.json`, and
  `~/.config/opencode/plugins/lade-pretool.js`.
- **Organic versus unknown via** ([#192](https://github.com/zifeo/lade/pull/192)): `detect()` treats a TTY inject with no
  `--pretool` and no agent signal as organic human, and a non-TTY empty via
  as unknown. The child never inherits `LADE_VIA`.
- **MCP stdio restart** ([#195](https://github.com/zifeo/lade/pull/195)): if the
  stdio child exits before the client `initialize` line is forwarded, Lade
  respawns it with the already hydrated env instead of calling vaults again.
  After `initialize`, a child exit does not restart.
- **T protocol**: first match writes `{temp}/lade-t/{id}.json`. The wrap
  and `lade set` hydrate from that file. No rematch. `set` writes T on
  every match and exports `LADE_T`. The ticket holds `via`,
  `network_pids`, and `pending`. Env keeps `LADE_T`, `LADE_RESTORE`,
  and `LADE_APPROVE`. Pretool wrap unlinks after the child. `lade unset`
  reads pids from T, unlinks, and clears `LADE_T`. Diary `log` is last
  explicit `log` on matching rules. No-match `seen` uses the last
  explicit `log` on the loaded walk. See `docs/protocol.md` and
  `docs/observability.md`.
- **Diary `agent` object**: free-form JSON on each event (`harness`,
  `model`, `session`, plus later keys). Hook payload wins over env.
  Missing or unknown fields stay absent. Empty objects are omitted.
- **`lade hook --harness`**: each installed hook names its host
  (`cursor`, `claude`, `codex`, `pi`, `opencode`). Unknown values are
  ignored. Detect and ticket writes fail open so a harness change does
  not block the tool call.
- **Daily hook refresh**: the same 24h window as the GitHub version
  check rewrites already-installed hook files when the command is
  stale. It never creates a hook the user did not install. `lade
  upgrade` clears `update_check` and `self_version` so the first run
  of the new binary checks GitHub and refreshes those files.

### Changed

- **`lade install` terms**: pre-exec is this shell only (other shells
  are listed, not installed). pre-tool is agents, hook and skill
  together. One box for both jobs. `lade uninstall` uses the same
  default plane as install, then the other plane if that one is empty.
  Drift verbs are `current`, `updated`, `installed`. Paths are
  repo-relative or `~/…`.
- **Lade skill**: pointer only. Run the command normally. Never `lade
  eval`, `--no-mask`, or `lade approve`. On missing or drifted hooks,
  run `lade install`. On withheld access, stop. Previous official
  skills stay Lade-managed and refresh to this text.
- **Single rustls stack**: CLI `reqwest` and `self_update` use rustls
  like the SDK. Dropped vendored OpenSSL, `path-clean`, `sysinfo`, and
  unused zip/bzip2 codecs on `self_update`. Parent shell detect reads
  the parent comm on Linux/macOS when `LADE_SHELL` is unset.
- **Tunnel providers in `lade-sdk`**: network CLI specs, kube context
  resolve, and command builders live with URI parse. The CLI still
  owns acquire, PTY, and process groups.
- **`self_update` 1.3**: first stable crate line. GitHub tags `v9.x` are
  example fixtures, not crate versions. `compression-flate2` is now
  `compression-tar-gz`. TLS stays `native-tls` (1.0 defaulted to
  rustls). `lade upgrade` uses `release_tag` and `ReleaseStatus`.
- **`lade status --json` hooks object** (breaking): `hooks` is now
  `{ "preexec": { shell, profile, installed, inject_skips_startup_files, inject_startup_skipped }, "pretool": { cursor, claude, codex, pi, opencode } }`
  with global and project paths. `ok` still depends only on preexec install,
  version, project config, and vault CLIs.
- **UI mode**: `Hook` is renamed `Quiet`. Interactive only when a human
  `inject`/`approve` has both stdin and stderr as TTYs.
- **OpenCode plugin**: `lade hook --harness opencode` takes
  `{ command, session_id }` and returns `{ command }`. No Claude
  envelope, no `OPENCODE=1`.
- **`--pretool`**: `lade hook` rewrites matching commands to
  `<lade> --pretool=<id> '…'` (the inject alias). The id is a 4-character
  T ticket. `--pretool` wins over the subcommand. Via is stored on the
  ticket. The child never sees `LADE_T` or `LADE_VIA`.
- **Protocol env**: `LADE_T` is the preexec pointer. `LADE_RESTORE` is
  the previous env. `LADE_APPROVE` is `sha256(command + window)[:5]`.
  `LADE_VIA`, `LADE_NETWORK_PIDS`, `LADE_PENDING`, and
  `LADE_DISCLAIMER_APPROVED` are no longer classifiers or messengers.
- **No stale protocol broom**: `set` / `unset` no longer emit `unset`
  for dropped keys. `unset` stops tunnels from the T ticket only.
- **Hook bin name**: `lade install` and match rewrites use argv[0]. `lade`
  stays `lade`. A path invocation (`/opt/lade install`) uses `current_exe`.
  Re-running `lade install` updates an existing hook that still has a path.

### Fixed

- **`.when` ignores `CURSOR_VERSION`**: Cursor sets it in human terminals, so it
  must not select agent rules on `inject` / `mcp`.
- **preTool envelope**: `CURSOR_VERSION` no longer wins over a `PreToolUse`
  payload or an explicit Codex, Pi, or OpenCode detect path. Those hosts ignore
  Cursor's `updated_input`.
- **Already-injected hook rewrite**: stamp `--pretool` when the command is
  already `lade inject` or `lade --pretool` without that flag, so `.when: agent`
  still matches. Older `--via=pretool` and `LADE_VIA=pretool` prefixes still
  count as stamped.
- **`lade status` project preTool paths** walk toward `$HOME` and stop there, so
  home-level agent hook files stay in the global slot.
- **`lade status` latest version**: persist the GitHub tag from a successful
  daily check so `status` can show it after shell use. If the fetch failed,
  print when we last tried (`tried today at 14:25`, `tried yesterday at 09:05`)
  instead of `not checked recently`.
- **Wrap profile overwrite** ([#194](https://github.com/zifeo/lade/pull/194)):
  `lade inject` and `lade hook` wrap fish with `--no-config` and zsh with `-f`,
  and clear `$BASH_ENV`, so a user profile cannot overwrite resolved secrets
  after spawn. `sh://` / `fish://` / `zsh://` use the same argv. Preexec
  (`lade set`) still evals in the live interactive shell. `lade status`
  reports `inject wrap: skips startup files` and names `config.fish`,
  `.zshenv`, or `BASH_ENV` when present.
- **Child signals** ([#195](https://github.com/zifeo/lade/pull/195)): `lade mcp`
  and `lade inject` share Unix signal wrapping. Hangup is ignored. Stop
  signals (`INT`/`TERM`/`QUIT`) end the wrapper. `USR1`/`USR2`/`WINCH` are
  forwarded. MCP children get their own session and process group. Inject
  stays on the TTY session.

## [0.17.2] - 2026-08-13

### Fixed

- **Shell hook stall** ([#182](https://github.com/zifeo/lade/pull/182)): stamp
  and time out the upgrade check, export `LADE_SHELL` from `lade on` so `set`
  skips a full process scan, and unregister hooks without leaving empty array
  slots.

[0.17.2]: https://github.com/zifeo/lade/compare/v0.17.1...v0.17.2

## [0.17.1] - 2026-08-02

### Fixed

- **Network tunnel restart** ([#180](https://github.com/zifeo/lade/pull/180)):
  supervise command-scoped forwards and restart a provider that exits during
  the command, instead of leaving the local port dead.

[0.17.1]: https://github.com/zifeo/lade/compare/v0.17.0...v0.17.1

## [0.17.0] - 2026-07-31

### Added

- **`lade mcp`** ([#171](https://github.com/zifeo/lade/pull/171)): resolve the
  matching `lade.yml` rule for a local stdio MCP server or a remote Streamable
  HTTP endpoint. Public bindings become child env vars or HTTP headers.
  Intermediate `.NAME` bindings stay resolver-local.
- **Binding DAG** ([#171](https://github.com/zifeo/lade/pull/171)): bindings
  can reference each other with `$NAME` / `${NAME}` / `${.NAME}`. Ready
  providers resolve concurrently. Shell providers receive those values as
  environment variables without rewriting the script.

### Fixed

- **Teleport `tsh` URIs** ([#172](https://github.com/zifeo/lade/pull/172)):
  accept `/app/<name>[/<target-port>]` and
  `/kube_cluster/<cluster>/<namespace>/<kind>/<name>/<remote-port>`. Loopback
  local binds stay on `127.0.0.1`.

[0.17.0]: https://github.com/zifeo/lade/compare/v0.16.0...v0.17.0

## [0.16.0] - 2026-07-06

### Added

- **Ephemeral network providers** ([#168](https://github.com/zifeo/lade/pull/168)): command-scoped local forwards for `kubectl://`, `kubefwd://`, `tsh://`, and `ssh://` URIs. Network entries can export a dynamic local port into an environment variable or bind a fixed local port, and Lade cleans them up after `inject` exits or shell hooks run `unset`.
- **Shell command secret provider** ([#169](https://github.com/zifeo/lade/pull/169)): `sh://`, `bash://`, `zsh://`, and `fish://` URIs resolve a secret value from a command, then participate in the same masking and hydration flow as other providers.
- **Lade Cursor skill** ([#168](https://github.com/zifeo/lade/pull/168)): project skill for coding agents that need to run commands requiring secrets or temporary network access.
- **Network and shell-provider coverage** ([#168](https://github.com/zifeo/lade/pull/168), [#169](https://github.com/zifeo/lade/pull/169)): Docker/k3d integration tests, shell-provider tests, and new README demo recordings.

### Changed

- **Provider lifecycle** ([#168](https://github.com/zifeo/lade/pull/168)): secret hydration and network acquisition now share a provider registry/progress path, so shell hooks and `lade inject` acquire the same command-scoped resources before running a matched command.
- **README and architecture docs** ([#168](https://github.com/zifeo/lade/pull/168)): refreshed the product framing around command-scoped secrets, temporary files, private-network access, and AI-agent workflows; added network-provider and shell-provider references.

### Removed

- **Windows support** ([#168](https://github.com/zifeo/lade/pull/168)): Lade now targets Unix only (macOS, Linux). Ephemeral port forwarding relies on POSIX process groups/signals (`setsid`, `killpg`) and interactive masking relies on PTYs. The Windows CI matrix, Windows release asset, Windows installer branch, and Windows `setup-lade` action step were removed. Use WSL or a Linux/macOS host on Windows.

[0.16.0]: https://github.com/zifeo/lade/compare/v0.15.3...v0.16.0

## [0.15.3] - 2026-06-14

### Changed

- **"Lade loaded" message** ([#156](https://github.com/zifeo/lade/pull/156)): prints as a plain line on TTY stderr instead of a framed message-box line.
- **README demos** ([#156](https://github.com/zifeo/lade/pull/156)): terminal recordings now render with asciinema and agg instead of VHS tape files.

[0.15.3]: https://github.com/zifeo/lade/compare/v0.15.2...v0.15.3

## [0.15.2] - 2026-06-13

### Added

- **Disclaimer approval flow** ([#154](https://github.com/zifeo/lade/pull/154)): `lade approve` command to review and accept disclaimers when using shell hooks.
- **Hook short-circuit** ([#154](https://github.com/zifeo/lade/pull/154)): shell hooks now skip any command starting with `lade` to avoid recursion and unnecessary overhead.

### Changed

- **Disclaimer in hooks** ([#154](https://github.com/zifeo/lade/pull/154)): when a disclaimer is required in hook mode, Lade now withholds secrets and exports `LADE_PENDING` instead of just failing.

[0.15.2]: https://github.com/zifeo/lade/compare/v0.15.1...v0.15.2

## [0.15.1] - 2026-06-06

### Added

- **Architecture documentation** ([#152](https://github.com/zifeo/lade/pull/152)): `docs/architecture.md` overview of shell hooks, config resolution, and secret injection.

### Changed

- **Message box** ([#152](https://github.com/zifeo/lade/pull/152)): "Action" tone renamed to **Info** (blue); upgrade nudges use Info instead of Warning.
- **TTY-aware UI** ([#152](https://github.com/zifeo/lade/pull/152)): framed warnings, "Lade loaded" lines, loader-error wait, and CLI compatibility prompts only on interactive stderr; piped or non-TTY runs stay quiet.
- **Upgrade nudge** ([#152](https://github.com/zifeo/lade/pull/152)): Enter runs `lade upgrade -y` inline; prompt auto-continues after 5s; background check limited to `inject` and `set`.
- **Snooze prompts** ([#152](https://github.com/zifeo/lade/pull/152)): clearer labels and 5s timeout; Ctrl+C dismisses instead of aborting the command.
- **Command rule matching** ([#152](https://github.com/zifeo/lade/pull/152)): `RegexSet` for faster lookup when many rules are defined in `lade.yml`.

### Fixed

- **Disclaimer in non-interactive shells** ([#152](https://github.com/zifeo/lade/pull/152)): exit with a hint to use `lade inject` instead of blocking on stdin.
- **Ctrl+C on optional prompts** ([#152](https://github.com/zifeo/lade/pull/152)): snooze and upgrade nudges treat Ctrl+C as dismiss, not exit 130.
- **Upgrade after disclaimer abort** ([#152](https://github.com/zifeo/lade/pull/152)): skip upgrade prompt when the user cancelled a disclaimer.

[0.15.1]: https://github.com/zifeo/lade/compare/v0.15.0...v0.15.1

## [0.15.0] - 2026-06-06

### Added

- **Disclaimer prompts** ([#143](https://github.com/zifeo/lade/pull/143)): optional `disclaimer` in the `.` rule block shows a framed warning before `inject` / `set`; the user must type `yes` to continue (Ctrl+C aborts without injecting secrets).
- **Message box** ([#143](https://github.com/zifeo/lade/pull/143)): shared stderr UI for disclaimers, config parse errors, upgrade nudges, and CLI compatibility warnings.
- **CLI compatibility warnings** ([#150](https://github.com/zifeo/lade/pull/150)): on `inject`, `set`, and `eval`, detect vault CLIs (1Password, Doppler, Vault, Infisical, Passbolt) older than the minimum versions Lade is tested against; framed warning with install links; snooze 1h / 24h / 7d.

### Changed

- Config parse failures now render through the message box with a format hint instead of a plain error line.
- **`inject` output masking** ([#149](https://github.com/zifeo/lade/pull/149)): only applies to secrets resolved by vault/file loaders; raw-loader inline values are no longer masked (they are already visible in `lade.yml`, and masking short literals such as API version numbers broke unrelated command output).
- **Upgrade nudge** ([#150](https://github.com/zifeo/lade/pull/150)): `lade upgrade` availability message uses the message box; optional snooze (1h / 24h / 7d) like CLI warnings.
- **README** ([#149](https://github.com/zifeo/lade/pull/149)): refreshed feature demos and documentation layout.

[0.15.0]: https://github.com/zifeo/lade/compare/v0.14.4...v0.15.0

## [0.14.4] - 2026-04-23

### Fixed

- **PTY handling** ([#140](https://github.com/zifeo/lade/pull/140)): correct pseudo-terminal behavior for injected commands (interactive tools, pagers, etc.).

[0.14.4]: https://github.com/zifeo/lade/compare/v0.14.3...v0.14.4

## [0.14.3] - 2026-04-21

### Fixed

- **Duplicate secret values** ([#138](https://github.com/zifeo/lade/pull/138)): redaction and hydration when multiple variables resolve to the same value.

[0.14.3]: https://github.com/zifeo/lade/compare/v0.14.2...v0.14.3

## [0.14.2] - 2026-04-18

[0.14.2]: https://github.com/zifeo/lade/compare/v0.14.1...v0.14.2

## [0.14.1] - 2026-04-18

[0.14.1]: https://github.com/zifeo/lade/compare/v0.14.0...v0.14.1

## [0.14.0] - 2026-04-18

### Added

- **`lade eval <uri>`** ([#132](https://github.com/zifeo/lade/pull/132)): resolve a single secret URI and print the value (debugging and scripting).
- **Secret redaction on `inject`** ([#135](https://github.com/zifeo/lade/pull/135)): mask secret values in stdout/stderr with self-rehydrating bash tokens (`${VAR:-REDACTED}` by default); `--no-mask` and `--mask-format` flags.
- **Agent hooks** ([#131](https://github.com/zifeo/lade/pull/131)): `lade hook` for Cursor and Claude Code shell tool pre-hooks.
- **`LADE_LOG`** ([#133](https://github.com/zifeo/lade/pull/133)): standard `env_logger` filter syntax instead of only `-v` flags.
- **Improved config errors** ([#133](https://github.com/zifeo/lade/pull/133)): clearer messages when `lade.yml` fails to parse.

### Changed

- **Architecture refactor** ([#136](https://github.com/zifeo/lade/pull/136)): internal simplification; `lade_sdk` crate for hydration logic.

### Fixed

- **Secret names with `+`** ([#134](https://github.com/zifeo/lade/pull/134)): correct handling in vault/loader paths.

[0.14.0]: https://github.com/zifeo/lade/compare/v0.13.0...v0.14.0

## [0.13.0] - 2026-02-18

### Added

- **Per-user secrets** ([#116](https://github.com/zifeo/lade/pull/116)): map usernames to different values; `"."` as default; `lade user` subcommand.
- **1Password service account** ([#123](https://github.com/zifeo/lade/pull/123)): `1password_service_account` in `.` block resolves `OP_SERVICE_ACCOUNT_TOKEN` from any loader (including cross-vault).
- **`lade upgrade`** ([#123](https://github.com/zifeo/lade/pull/123)): self-update from GitHub releases; background update nudge on other commands.
- **Verbose 1Password errors** ([#118](https://github.com/zifeo/lade/pull/118)).

### Fixed

- **Upgrade during command** ([#117](https://github.com/zifeo/lade/pull/117)): avoid breaking the running command when a new version is detected.

[0.13.0]: https://github.com/zifeo/lade/compare/v0.12.1...v0.13.0

## [0.12.1] - 2025-06-07

### Fixed

- **Non-Unicode output** ([#110](https://github.com/zifeo/lade/pull/110)): panic when subprocess lines are not valid UTF-8.

[0.12.1]: https://github.com/zifeo/lade/compare/v0.12.0...v0.12.1

## [0.12.0] - 2025-05-19

### Added

- **Infisical nested paths** ([#106](https://github.com/zifeo/lade/pull/106)): support secrets under nested folder paths.

### Changed

- **Test scripts** ([#105](https://github.com/zifeo/lade/pull/105)): automatic `scripts/test.*` runs in CI.

[0.12.0]: https://github.com/zifeo/lade/compare/v0.11.5...v0.12.0

## [0.11.5] - 2024-10-31

[0.11.5]: https://github.com/zifeo/lade/compare/v0.11.4...v0.11.5

## [0.11.4] - 2024-10-31

[0.11.4]: https://github.com/zifeo/lade/compare/v0.11.3...v0.11.4

## [0.11.3] - 2024-08-28

[0.11.3]: https://github.com/zifeo/lade/compare/v0.11.2...v0.11.3

## [0.11.2] - 2024-05-04

[0.11.2]: https://github.com/zifeo/lade/compare/v0.11.1...v0.11.2

## [0.11.1] - 2024-04-25

[0.11.1]: https://github.com/zifeo/lade/compare/v0.11.0...v0.11.1

## [0.11.0] - 2024-04-11

### Added

- **Passbolt loader** ([#70](https://github.com/zifeo/lade/pull/70)): `passbolt://DOMAIN/RESOURCE_ID/FIELD`.

[0.11.0]: https://github.com/zifeo/lade/compare/v0.10.0...v0.11.0

## [0.10.0] - 2024-03-08

### Added

- **Vault URL decoding** ([#62](https://github.com/zifeo/lade/pull/62)): URL-decode mount/key/field segments.
- **1Password file & multiline** ([#67](https://github.com/zifeo/lade/pull/67)): file attachments and multiline fields from 1Password.

[0.10.0]: https://github.com/zifeo/lade/compare/v0.9.1...v0.10.0

## [0.9.1] - 2023-10-02

[0.9.1]: https://github.com/zifeo/lade/compare/v0.9.0...v0.9.1

## [0.9.0] - 2023-10-02

### Added

- **`lade inject`** ([#51](https://github.com/zifeo/lade/pull/51)): manual secret injection for scripts and non-interactive shells.
- **Absolute paths for file output** ([#50](https://github.com/zifeo/lade/pull/50)).

### Fixed

- **Bash on/off** ([#49](https://github.com/zifeo/lade/pull/49)): shell hook toggling in Bash.

[0.9.0]: https://github.com/zifeo/lade/compare/v0.8.1...v0.9.0

## [0.8.1] - 2023-08-10

[0.8.1]: https://github.com/zifeo/lade/compare/v0.8.0...v0.8.1

## [0.8.0] - 2023-08-03

### Added

- **Error banner** ([#42](https://github.com/zifeo/lade/pull/42)): surface vault/loader failures instead of failing silently.

[0.8.0]: https://github.com/zifeo/lade/compare/v0.7.0...v0.8.0

## [0.7.0] - 2023-06-13

### Added

- **Secrets as files** ([#36](https://github.com/zifeo/lade/pull/36)): `file` in `.` block writes YAML/JSON (and related formats) instead of env vars.

[0.7.0]: https://github.com/zifeo/lade/compare/v0.6.2...v0.7.0

## [0.6.2] - 2023-05-20

[0.6.2]: https://github.com/zifeo/lade/compare/v0.6.1...v0.6.2

## [0.6.1] - 2023-05-19

[0.6.1]: https://github.com/zifeo/lade/compare/v0.6.0...v0.6.1

## [0.6.0] - 2023-05-05

### Added

- **Shell auto-launcher** ([#31](https://github.com/zifeo/lade/pull/31)): `lade install` / `on` / `off` hooks for Fish, Bash, Zsh.
- **Whitespace in commands** ([#30](https://github.com/zifeo/lade/pull/30)): regex keys and command matching improvements.

[0.6.0]: https://github.com/zifeo/lade/compare/v0.5.5...v0.6.0

## [0.5.5] - 2023-04-21

[0.5.5]: https://github.com/zifeo/lade/compare/v0.5.4...v0.5.5

## [0.5.4] - 2023-04-18

[0.5.4]: https://github.com/zifeo/lade/compare/v0.5.3...v0.5.4

## [0.5.3] - 2023-04-06

[0.5.3]: https://github.com/zifeo/lade/compare/v0.5.2...v0.5.3

## [0.5.2] - 2023-04-04

[0.5.2]: https://github.com/zifeo/lade/compare/v0.5.1...v0.5.2

## [0.5.1] - 2023-04-04

[0.5.1]: https://github.com/zifeo/lade/compare/v0.5.0...v0.5.1

## [0.5.0] - 2023-04-02

### Added

- **File loader** ([#15](https://github.com/zifeo/lade/pull/15)): `file://` URIs with JSONPath-style queries (INI, JSON, YAML, TOML).

[0.5.0]: https://github.com/zifeo/lade/compare/v0.4.0...v0.5.0

## [0.4.0] - 2023-03-16

### Added

- **HashiCorp Vault loader** ([#13](https://github.com/zifeo/lade/pull/13)): `vault://` URIs.

[0.4.0]: https://github.com/zifeo/lade/compare/v0.3.1...v0.4.0

## [0.3.1] - 2023-03-07

[0.3.1]: https://github.com/zifeo/lade/compare/v0.3.0...v0.3.1

## [0.3.0] - 2023-03-07

[0.3.0]: https://github.com/zifeo/lade/compare/v0.2.2...v0.3.0

## [0.2.2] - 2023-03-04

[0.2.2]: https://github.com/zifeo/lade/compare/v0.2.1...v0.2.2

## [0.2.1] - 2023-02-28

[0.2.1]: https://github.com/zifeo/lade/compare/v0.2.0...v0.2.1

## [0.2.0] - 2023-02-26

[0.2.0]: https://github.com/zifeo/lade/compare/v0.1.3...v0.2.0

## [0.1.3] - 2023-02-20

[0.1.3]: https://github.com/zifeo/lade/releases/tag/v0.1.3
