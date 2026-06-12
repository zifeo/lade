# Changelog

All notable changes to [Lade](https://github.com/zifeo/lade) are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Release notes are also published on [GitHub Releases](https://github.com/zifeo/lade/releases).

## [Unreleased]

### Added

- **Disclaimer approval flow**: `lade approve` command to review and accept disclaimers when using shell hooks.
- **Hook short-circuit**: shell hooks now skip any command starting with `lade` to avoid recursion and unnecessary overhead.

### Changed

- **Disclaimer in hooks**: when a disclaimer is required in hook mode, Lade now withholds secrets and exports `LADE_PENDING` instead of just failing.

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
