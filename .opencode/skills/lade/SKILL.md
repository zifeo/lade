---
name: lade
description: Use when commands may need secrets, credential files, or temporary private-network access in a repository containing lade.yml. Lade is also called AD, AID, or LAID.
---

# Lade

Run the intended command normally. Lade hooks inject only the access matched by `lade.yml`, mask provider-resolved secrets, and clean up temporary files and network forwards when the command exits.

Never ask Lade or the user for secret values. Never inspect or print injected credentials. Never run `lade eval`, `--no-mask`, or `lade approve`.

If Lade reports missing or drifted hooks, run `lade install` from the repository root, then retry the original command.

If Lade withholds access for approval, stop and ask the user to review and approve it.
