# Environment variables

Most Lade environment variables are internal. They wire the shell
protocol, pretool tickets, and tests. Override the global config path
when you need a non-default home for `config.json`.

See also [protocol.md](protocol.md) (T tickets), [architecture.md](architecture.md)
(disclaimer and wrap flow), [observability.md](observability.md)
(diary), and [cache.md](cache.md) (hub).

## User overrides

| Variable | Purpose |
| --- | --- |
| `LADE_CONFIG_PATH` | Path to the global config file (`config.json`). Default: XDG config dir. Tests and CI set this to avoid update checks. |
| `LADE_CACHE_DIR` | Override the hub socket / ticket / mise-env root. Tests and tape render set this. |
| `LADE_DAEMON` | `off` skips the in-memory hub. No connect, no spawn. |
| `LADE_WRAP_KEY` | 64 hex chars (32 bytes). Tests and CI only. Replaces Keychain / `@s`. Bad hex skips the hub. |

## Installer and CI

Used by `installer.sh` and the GitHub Action. Not needed for day-to-day
`lade setup` in a repo.

| Variable | Purpose |
| --- | --- |
| `OUT_DIR` | Install directory (both binaries added to `PATH`). |
| `VERSION` | Release version without `v`, or empty for latest. |
| `PLATFORM` | Override `OS-ARCH` asset name. |
| `ASSUME_YES` | Non-interactive install (`1` / `true`). |
| `CI` | Skip shell-wrap prompts; same as `ASSUME_YES` for the installer. |
| `DOWNLOADER` | Force `curl` or `wget`. |

## Internal protocol

Set by Lade or the shell hook. Do not rely on these in scripts.

| Variable | Purpose |
| --- | --- |
| `LADE_T` | Active pretool / pre-exec ticket id (4-char). Points at `{cache}/tickets/{id}.json`. |
| `LADE_RESTORE` | Previous env snapshot for pre-exec cleanup. May contain secrets. |
| `LADE_APPROVE` | Per-command disclaimer code (`sha256(command + window)[:5]`). Prefix the command or run `lade approve`. |
| `LADE_BIN` | `age-plugin-lade` uses this to find `lade eval` when not on `PATH`. |
| `LADE_EVENTS` | `off` disables diary writes. |
| `LADE_EVENTS_PATH` | Override diary database path (tests). |
| `LADE_TICKET_DIR` | Override pretool ticket directory (tests). |
| `LADE_LOG` | `env_logger` filter for debug builds. |
| `LADE_SHELL` | Shell family hint (`bash`, `zsh`, `fish`) when detection is ambiguous. |

Leftover `LADE_VIA` is ignored. Via and audience are chosen per
invocation, not from the environment.

## Harness detection

When Via is unknown, Lade may classify the invocation as harness
audience (`when: agent` rules) from env signals. First match wins
(`src/audience/mod.rs`):

| Signal | Harness |
| --- | --- |
| `AI_AGENT`, `AGENT` | value of the variable |
| `CLAUDECODE=1`, `CLAUDE_CODE` | claude |
| `CURSOR_AGENT`, `CURSOR_EXTENSION_HOST_ROLE=agent-exec`, `CURSOR_SANDBOX` | cursor |
| `CODEX_THREAD_ID`, `CODEX_SANDBOX`, `CODEX_CI` | codex |
| `OPENCODE`, `OPENCODE_PID` | opencode |
| `COPILOT_MODEL` | copilot |

`CURSOR_VERSION` is **not** a signal (Cursor sets it in human terminals
too). Do not set these unless you emulate a harness in tests.
