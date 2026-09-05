# AGENTS.md

Conventions for AI agents contributing to **lade** (a Rust CLI). For *using*
lade with coding agents and CI, see the README — this file is about working on
the codebase itself.

## Build & test

Run these before proposing changes; they must all pass:

```bash
cargo build --workspace --locked
cargo test --workspace --locked
cargo clippy --all-targets -- -D warnings
shellcheck installer.sh
bash tests/installer_test.sh
```

## House rules

- **All user-facing stderr goes through `message_box::MessageBox`** — never
  `eprintln!`. The box is always emitted; only interactive parts (prompts,
  countdowns, sleeps) are gated on `UiMode` (`Quiet` vs `Interactive`). See
  `.cursor/rules/message-box.mdc`.
- Never rely on default values; be explicit. Prefer the simplest solution that
  compiles. Comment only non-obvious intent, not what the code does.
- Keep documented exit codes (`src/exit_codes.rs`) stable across minor
  versions. `lade status --json` keeps `version`, `global_config`, `hooks`,
  `project_config`, and `ok`; the `hooks` object is `preexec` plus `pretool`.
- **`lade status` latest**: a successful daily GitHub check must persist
  the tag (`latest_version` in the global config) so status can show it after
  shell use. If the fetch failed, print when we last tried (`tried today at
  14:25`, `tried yesterday at 09:05`). Do not print `not checked recently`
  when a check was attempted. `version` in `--json` includes `latest`,
  `last_check`, `update_available`, and `check_error`.

## Project layout

- `src/` — CLI crate. Key modules: `pretool/` (`lade hook` preToolUse handler
  plus install into Cursor/Claude/Codex/Pi/OpenCode configs), `audience.rs` (`detect()` for Via,
  Audience, UI), `prompt.rs` (disclaimer flow), `inject.rs`/`exec/` (PTY
  execution + masking), `status.rs`, `shell/` (preexec integration), `config/`,
  `message_box/`.
- `sdk/` — the secret-loader crate (vault providers).
- `tests/` — Rust integration tests + `installer_test.sh`.
- `scripts/`, `examples/tape/` — shell-hook fixtures and README demo tapes.
- `installer.sh`, `action.yml`, `Dockerfile`, `.github/workflows/` —
  install & CI surface.

## Maintainer note

The GitHub Action (`action.yml`) is **not** auto-published to the Marketplace:
on each release, tick "Publish this Action to the GitHub Marketplace" in the
release UI once.
