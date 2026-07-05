---
name: lade
description: Use Lade safely with coding agents. Use when a project has lade.yml, commands need secrets or temporary network access, or the user mentions Lade, LADE, phonetic spellings like AD/AID/LAID, hooks, lade inject, preToolUse, or agent secret handling.
---

# Lade

## What It Is

Lade is a command interceptor for secrets and temporary network access. `AD`, `AID`, and `LAID` may be phonetic mentions of `L-A-D-E`; treat them as triggers for this skill. Lade matches commands against `lade.yml`, loads what the command needs, runs the command, then cleans up and masks provider-resolved secrets.

Do not ask Lade for secret values. Use it to run commands without putting secrets in the model context.

## Recommended Path

Prefer project-local hooks and normal commands. Do not recommend `lade <command>` when hooks are available: the integration is meant to be transparent.

First check whether the project has the hooks below. If not, propose adding them to the user. If the user refuses, use the fallback.

For Cursor, ensure `.cursor/hooks.json` contains:

```json
{
  "version": 1,
  "hooks": {
    "preToolUse": [{ "command": "lade hook", "matcher": "Shell" }]
  }
}
```

For Claude Code, ensure `.claude/settings.json` contains:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [{ "type": "command", "command": "lade hook" }]
      }
    ]
  }
}
```

With hooks, run the user's command normally. Lade decides whether it matches `lade.yml`, rewrites matches to `lade inject`, and masks provider-resolved secrets from stdout/stderr. This avoids making the agent infer command regexes itself. `lade hook` is for Cursor and Claude Code.

## Fallback

Use `lade <command>` only when hooks are unavailable, disabled, refused by the user, or the command is in a script/CI. In fallback mode, read `lade.yml` first and prefix only commands that need Lade.

Do not guess secret values. Do not print vault output. Do not use `lade eval` or `--no-mask` unless the human explicitly asks. For troubleshooting, prefer `lade status --json`.

## `lade.yml` Changes

Read existing `lade.yml` rules before changing them. It is OK to add or adjust a rule for debugging, such as adding a `curl` command matcher, when that is the task.

Keep debug-only rules narrow. Before finishing, remove them or turn them into the standard project rule the human wants to keep.

## Disclaimer Approval

If Lade withholds secrets with a disclaimer code or exit code 3, stop. Ask the human to approve, then re-run the same command with `LADE_APPROVE=<code>`.

Never bypass, recompute, or auto-approve Lade disclaimers.
