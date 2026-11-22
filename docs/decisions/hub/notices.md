# Notices and status

A person should hear about a new release and about hooks that need them, without a box or a network call on every command, and an operator must see whether the hub is up and may cache. This note binds when the hub's notices show, how the release check runs, and what `lade status` says of the hub.

## Notices

A person should hear about a new Lade release and about pre-tool hooks that need them, but a box on every command is noise, and a network call on every command is cost. The hub hands out an hourly slot to commands that talk to it anyway: "from `lade set`, only for a human at a terminal, and never a hub call of their own: a plain command stays off the hub. A command that uses the hub (secrets, tunnels, disclaimers, pinned tools) asks for the hourly slot in its first `PutT`" (`AGENTS.md`, House rules, Shell notices; `docs/cache.md`, Who talks to it: "that `PutT` may spawn the hub, so a repo that only pins packages still gets checks"). stdout of `lade set` is the shell protocol, so the notice goes on stderr and only to a terminal. "The slot owner refreshes hook files, then shows at most one box: a new release, and hooks that need the user" (`docs/cache.md`). What the hook refresh writes, and the check on every harness start, is [khcwz2ec3](../harness/install.md#khcwz2ec3).

An agent cannot upgrade Lade or fix a hook, but its human can: "A pre-tool wrap that uses the hub gets the same slot" (CHANGELOG 0.20.0, Release check and notices), and its stderr line reads "`lade: Tell the human: <notice>`" (`docs/cli.md`, What an agent reads; CHANGELOG 0.20.0, What an agent reads).

The period is an hour (`AGENTS.md`, "hourly"). The sources give a reason for a slot, not for an hour, so the law binds a fixed period. The hook box a shell shows when it starts a harness has no slot and is [khcwz2ec3](../harness/install.md#khcwz2ec3)'s. A pre-tool wrap whose rules only cancel keys has nothing for the hub, so it asks for no slot and starts no hub (`a_pretool_wrap_that_only_cancels_keys_starts_no_hub`).

**§ <a id="jpm1fzdj4">jpm1fzdj4</a> Lade shows the release and hook notices of the hub's slot at most once per fixed period per hub, and only on a command that already uses the hub.**

- (a) `lade set` asks for the slot only for a human whose stderr is a terminal, in the first ticket it sends the hub, which may start the hub.

- (b) A pre-tool wrap that prepares secrets, tunnels or a disclaimer asks for the same slot, and its notice tells the agent to pass it to the human.

- (c) The hub gives the slot to one caller per period, and that caller shows at most one box, holding the newer release and the hooks that need the user.

## The release check

"The hub runs the GitHub check (at most daily, started by a human shell command that uses the hub) and keeps the tag in RAM, one ledger per Lade binary ... Status reads the ledger without starting a hub. A failed fetch keeps the last known tag" (`AGENTS.md`, House rules, `lade status` latest; CHANGELOG 0.20.0: "`lade status`, `setup`, and `update` read the hub's last result and never call GitHub themselves"). One place asks, so a terminal full of shells makes one request per period, and no command waits on GitHub. The period is a day ("at most daily"), a value no source argues for, so the law names a fixed period. The attempt is stamped before the fetch, so concurrent shells start one (`src/hub/release.rs`). The fetch runs with a timeout off the hub's request loop: "a hub stuck on GitHub would never exit" (`src/command/upgrade/mod.rs`), and "No user waits on this fetch" (`src/hub/release.rs`). The timeout is 10 s in the code, a value no source gives a reason for, so the law names a fixed timeout. A ledger on disk would be shared by two installed versions racing on one stamp, so it stays in the hub's RAM (`src/command/upgrade/mod.rs`, `VersionStatus`), and a new hub checks again ([cbz2he75h.d](lifecycle.md#cbz2he75h)). `lade upgrade` itself asks GitHub for the latest release and downloads it; only its release line reads the ledger.

**§ <a id="d1d316x4b">d1d316x4b</a> Only the hub checks GitHub for a release notice, at most once per fixed period, and it keeps the answer in RAM.**

- (a) A notice request starts the fetch when the hub has made no attempt or its last attempt is a period old, and the attempt is recorded before the fetch starts.

- (b) The fetch runs off the hub's request loop and gives up after a fixed timeout.

- (c) A failed fetch keeps the last known tag and records its error.

- (d) `lade status`, `setup` and `update` read the release from the hub's ledger.

## What `lade status` shows

The operator's questions are whether the hub is up and whether it may cache at all: "`lade status` says which applies" (CHANGELOG 0.20.0, Who may read the cache). The Hub section shows "`pid …` or `down` / `stale`, the `max_cache` in force, whether the cache is on and what guards it (or why it is off, with the `codesign` command on macOS)" (`docs/cache.md`, CLI). The `hub` object in `--json` is extra, not one of the kept keys (`docs/architecture.md`: "`log`, `mise`, `age_plugin`, and `hub` are extra"); its fields are not a law.

"The hub never sets `ok`" (`docs/cache.md`, CLI): status reports this machine's setup, and a hub that is down is a normal state. Nor does the release check: "A failed GitHub check no longer makes it exit 1" (CHANGELOG 0.20.0, `lade status` layout), since "It says nothing about this machine's setup" (`src/command/status/gather.rs`). An invalid `max_cache` does set it. When `ok` is false is [vk7cw8j73.a](../observability/status.md#vk7cw8j73), which names neither the hub nor the release check. The latest-release line says when the hub last tried, as in this example: "`tried today at 14:25`, `tried yesterday at 09:05`. Do not print `not checked recently` when a check was attempted. `version` in `--json` includes `latest`, `last_check`, `update_available`, and `check_error`" (`AGENTS.md`, House rules), so a user can tell a failed check from one that never ran.

**§ <a id="rbzjhj8tk">rbzjhj8tk</a> `lade status` shows whether this binary's hub is up and whether it may cache secrets.**

- (a) The Hub section shows the hub's pid, or `down`, or `stale` for a hub that denied it.

- (b) It shows the memory guard in force, and when the cache is off, why, with the `codesign` command that turns it on for a macOS build.

- (c) **Retired.**

- (d) With no known tag, the latest line says when the hub last tried, and a failed check shows its error and when it was tried.

- (e) `version` in `--json` carries `latest`, `last_check`, `update_available` and `check_error`.
