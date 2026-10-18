# AGENTS.md

Conventions for AI agents contributing to **lade** (a Rust CLI). For *using*
lade with coding agents and CI, see the README — this file is about working on
the codebase itself.

## Build & test

Stay in the Cursor sandbox for `cargo` / `clippy` / tests. Do not
request `all` because of sccache. `enableSharedBuildCache` remaps
Cargo dirs. It does not cover the sccache daemon. Seatbelt denies
AF_UNIX / 127.0.0.1, so `.cargo/sccache-wrapper` skips sccache when
`CURSOR_SANDBOX` is set. `all` is not a compile workaround.

Run these before proposing changes; they must all pass:

```bash
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --all-targets -- -D warnings
shellcheck installer.sh
bash tests/installer_test.sh
```

## House rules

- **stderr: `MessageBox` for attention, `Report` for command results** —
  never `eprintln!` from command code. Box errors, warnings, disclaimers,
  pre-exec failures, and a new Lade release. `lade setup` / `lade update`
  success is a `Report` (bold headings, no border). See
  `.cursor/rules/message-box.mdc`. Interactive parts (prompts, countdowns,
  sleeps) stay gated on `UiMode`.
- Never rely on default values; be explicit. Prefer the simplest solution that
  compiles. Comment only non-obvious intent, not what the code does.
- Keep documented exit codes (`src/exit_codes.rs`) stable across minor
  versions. `lade status --json` keeps `version`, `global_config`, `hooks`,
  `project_config`, and `ok`; the `hooks` object is `preexec` plus `pretool`.
  `age_plugin` is extra (`present`, `path`). `mise`, `log`, and `hub` are extra.
  Spoken stderr, README, and clap say pre-exec / pre-tool. JSON stays
  `preexec` / `pretool`. `--global` and JSON `global` stay.
- User-facing copy for Cursor/Claude/Codex/OpenCode says **harness**,
  not agent. CLI: `--harness`. JSON diary field `agent` is hook metadata.
  Spoken **pre-exec** / **pre-tool** map to JSON `preexec` / `pretool`.
- **`lade status` latest**: the hub runs the GitHub check (at most daily,
  started by a human shell command that uses the hub) and keeps the tag in
  RAM, one ledger per Lade
  binary. `config.json` holds only `user`, and unknown keys are written back
  for older binaries. Status reads the ledger without starting a hub. A
  failed fetch keeps the last known tag; print when we last tried (`tried
  today at 14:25`, `tried yesterday at 09:05`). Do not print `not checked
  recently` when a check was attempted. `version` in `--json` includes
  `latest`, `last_check`, `update_available`, and `check_error`.
- **Shell notices**: from `lade set`, only for a human at a terminal, and
  never a hub call of their own: a plain command stays off the hub. A
  command that uses the hub (secrets, tunnels, disclaimers, pinned tools)
  asks for the hourly slot in its first `PutT`. The slot covers a new
  release and every harness's pre-tool hooks. Starting a harness
  (`opencode`, `claude`, `codex`, `cursor-agent`, `agent`) checks that
  harness's hooks every time, without the hub (`pretool/install/health.rs`).
- **Fake samples stay fake:** docs, README, clap examples, tests, and
  fixtures never use a real customer name, a real home path, or a real
  repo path. Use `~/src/app`, `/Users/me`, `acme`. No `claryo`, `dahu`,
  `$HOME/...`, or a teammate path. No TELEPORT fixtures. No PII.

## Project layout

- `src/` — CLI crate. `command/` is the clap verb entry points.
  Thematic engines: `pretool/` (`lade hook` preToolUse / MCP verb
  handler plus `lade hook enable [--global]` into
  Cursor/Claude/Codex/OpenCode configs), `preexec/` (shell wrap),
  `wrap/` (inject + PTY exec + masking), `mcp/`, `mise/` (command-scoped
  pins via lade.lock / mise.lock and `$MISE_INSTALLS_DIR`), `network/`
  (acquire and process groups), `event/` (diary, `log`, `usage`, packs),
  `config/`, `audience.rs`, `prompt.rs`, `message_box/`.
- `crates/lade-sdk` — providers: vault hydrate, network URI parse, tunnel
  command builders, CLI version tables. MCP HTTP byte bridge.
- `crates/age-plugin-lade` — C2SP `age-plugin-lade` binary. Execs
  `lade eval` for hydrate. Not compiled into the `lade` binary.
- `tests/` — Rust integration tests + `installer_test.sh`.
- `scripts/`, `examples/tape/` — shell-hook fixtures and README demo tapes.
- `installer.sh`, `action.yml`, `Dockerfile`,
  `.github/workflows/` — install & CI surface.
