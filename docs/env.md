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
| `LADE_CONFIG_PATH` | Path to the global config file (`config.json`: the Lade user and `max_cache`). Default: XDG config dir. |
| `LADE_CACHE_DIR` | Override the hub socket / ticket / mise-env root. Tests and tape render set this. Without it and without a home directory, Lade stops with a box. |
| `LADE_MISE_FETCH` | `0` turns off the fetch of the official mise release: a verb that would fetch it, such as `lade setup`, then fails when no mise on `PATH` is in the supported range, and says how to install mise. |
| `LADE_MISE` | Path to a mise binary Lade tries before the one on `PATH`. Lade runs it only when its version is in the supported range. |

## Installer and CI

Used by `installer.sh` and the GitHub Action. Not needed for day-to-day
`lade setup` in a repo.

| Variable | Purpose |
| --- | --- |
| `OUT_DIR` | Install directory for both binaries. When it is not on `PATH`, the installer adds it in your login shell's profile, the one `lade setup` writes its hook to (`~/.bashrc`, `~/.zshrc`, `~/.config/fish/config.fish`, else `~/.profile`), on one line ending in `# lade-path-do-not-edit`. A second run, or a profile that already adds it, is left as it is. The GitHub Action adds it to `PATH` itself. |
| `VERSION` | Release version without `v`, or empty for latest. |
| `PLATFORM` | Override `OS-ARCH` asset name. |
| `ASSUME_YES` | Non-interactive install (`1` / `true`). |
| `CI` | Skip shell-wrap prompts; same as `ASSUME_YES` for the installer. Empty, `0` and `false` are not CI. |
| `DOWNLOADER` | Force `curl` or `wget`. |

## Internal protocol

Set by Lade or the shell hook. Do not rely on these in scripts.

| Variable | Purpose |
| --- | --- |
| `LADE_T` | Active pretool / pre-exec ticket id (4-char). Points at `{cache}/tickets/{id}.json`. |
| `LADE_RESTORE` | Previous env snapshot for pre-exec cleanup. May contain secrets. |
| `LADE_APPROVE` | Per-command disclaimer code: the first 5 hex digits of a `sha256` of the command and the 5-minute window ([protocol.md](protocol.md)). Prefix the command or run `lade approve`. |
| `LADE_BIN` | The `lade` that `age-plugin-lade` runs for `lade eval` and the OpenCode plugin for `lade hook`, ahead of the one next to the age plugin, the one the OpenCode plugin was written for, and the one on `PATH`. A set `LADE_BIN` that is not a file fails. |
| `LADE_EVENTS_PATH` | Override diary database path (tests). |
| `LADE_TICKET_DIR` | Override pretool ticket directory (tests). |
| `LADE_VAULT_HTTP` | Test seam: set to any value, in the environment or a rule's, the Vault provider calls `http://` instead of `https://`, so tests and the tape reach a local dev server. The token then travels in clear, so only `localhost`, `127.0.0.1` and `[::1]` are allowed, as for `lade mcp` over plain HTTP; any other Vault host fails. |
| `LADE_LOG` | `env_logger` filter (`debug`, `lade::hub=debug`). `lade::timing=info` logs how long each hydrate phase and provider took. |

Leftover `LADE_VIA` is ignored. The shell is always the process that started Lade ([protocol.md](protocol.md#shell)). Via and audience are chosen per
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
