---
name: lade
description: Use Lade safely with coding agents. Use when a project has lade.yml, commands need secrets or temporary network access, or the user mentions Lade, LADE, phonetic spellings like AD/AID/LAID, hooks, lade inject, preToolUse, or agent secret handling.
---

# Lade

Lade intercepts commands that match `lade.yml`. It loads secrets and temporary network from the project's providers (1Password, Vault, Doppler, files, shell, kubectl, ssh, …) and masks those values in output. It replaces `.env`, dotenv, `op run`, `vault kv get`, and pasting provider output into the chat.

`AD` / `AID` / `LAID` mean Lade. Never ask Lade for secret values.

The live surface is the binary, not this file. Start there.

## 1. Is it installed?

```bash
lade status --json
```

Read `ok`, `hooks.pretool`, `skills`, and `project_config.vault_clis`. A missing or stale hook means interception will not happen. A provider warning (1Password missing, old CLI) is a blocker: fix that, do not invent a side channel.

This machine (detects agents, asks): `lade install`.

Project hook, preferred for the team (default scope is project):

```bash
lade hook install --harness <cursor|claude|codex|opencode>
```

This machine, one harness: `--scope user`. Codex: trust the command in `/hooks`.

`apm install zifeo/lade#<tag>` is this skill only. It does not install hooks.

## 2. Run

Hook current: type the command normally. Do not prefix `lade`.

Hook missing, refused, or CI: `lade <command>` or `lade inject -- <command>`. Read `lade.yml` first. Prefix matches only.

No `lade eval` / `--no-mask` unless the human asks.

## 3. Ask

- Which rules does this tree actually use? Set `log: true` on those rules, then `lade usage --json`.
- What did we type? `lade log --json`.
- Why did a secret not load? `lade status --json` (`vault_clis.warnings`, hook `current`).

## Disclaimer

Exit 3 or `LADE_APPROVE=<code>`: stop. The human approves. Never invent or auto-approve.
