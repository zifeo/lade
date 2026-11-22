# Platforms

Lade hooks into shells, ships for operating systems and writes hooks for harnesses, and each of those lists is a promise a user reads before installing. This note states which hosts Lade supports, so that a host added in one place and not the others never ships half-supported.

## Where Lade runs

"Lade on Fish, Bash, or Zsh. macOS and Linux. Cursor, Claude Code, Codex, and OpenCode" (`README.md`, title). Windows was removed in 0.16.0 because tunnels rely on POSIX process groups and signals and masking relies on pseudo-terminals (`CHANGELOG.md`, 0.16.0, Removed). Each list lives in several places in the code, the shells the hook knows, the platforms the installer and the release build, the harnesses the installer writes, the hook reads and the shell checks on launch, and a list that grows in one place and not the others ships a half-supported host. `sh` runs a line for a task runner (`docs/protocol.md`, Shell) but is not a pre-exec shell.

**§ <a id="mqnfssecp">mqnfssecp</a> Lade supports the shells Fish, Bash and Zsh, the systems macOS and Linux, and the harnesses Cursor, Claude Code, Codex and OpenCode.**

- (a) Lade writes a pre-exec hook only for Fish, Bash or Zsh, and refuses any other shell.

- (b) Lade ships and installs for macOS and Linux only.

- (c) Every list of harnesses Lade writes hooks for, reads hook payloads from, or checks when one starts names exactly Cursor, Claude Code, Codex and OpenCode.

The release build's target list lives in `.github/workflows/release.yml`, YAML Silcrow does not parse, so (b) is marked in the installer alone and a target added to the workflow fails no check. The audience signals of the harness notes also know Copilot: that list tells an agent from a human, it writes, reads and checks no hook.
