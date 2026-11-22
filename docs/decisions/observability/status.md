# Status and bench

`lade status --json` is read by scripts and harnesses, so a key that moves breaks them silently, and an `ok` that turns false for something optional fails every CI job that checks it. This note binds what `ok` means, which keys the JSON keeps, and what `lade bench` times.

## What `ok` means in `lade status`

`lade status` is the inventory a user and a CI job read to know whether this machine can run this repo's commands. Its `ok`, and the exit 1 that goes with it, must mean exactly that. Several things it shows are optional by design: a CI machine has no pre-exec hook ("`CI` set skips the wrap", CHANGELOG 0.19.0, `lade setup` / `lade teardown`, which is when pre-exec, a newer release and the old vault CLI warnings left `ok`); a missing `age-plugin-lade` "does not flip `ok`" (CHANGELOG 0.19.0, age plugin); the diary and the hub "leave `ok` unchanged" (`docs/observability.md`, Store; CHANGELOG 0.18.0, Local command diary). What `ok` does depend on is what a command needs to run: a yaml that loads, the mise that setup pins with, and the locked tools installed. An invalid `max_cache`, or a `config.json` Lade cannot read or parse, also sets it, since the hub then keeps nothing and every wrap fails on the Lade user, [cfan04ksd.e](../cache/ttl.md#cfan04ksd); the hub's own state and the release check are outside that list, [vk7cw8j73.a](status.md#vk7cw8j73). The 0.18.0 line "`ok` still depends only on preexec install, version, project config, and vault CLIs" was replaced in 0.19.0.

**§ <a id="vk7cw8j73">vk7cw8j73</a> `lade status` reports `ok: false` only when this machine cannot run this repo's commands as configured, and then exits 1.**

- (a) `ok` is false exactly when the walk's yaml fails to load, the repo needs mise and the mise found is out of the supported range, a locked tool is not installed, `config.json` cannot be read or parsed, or `max_cache` is invalid.

- (b) Status exits 1 when `ok` is false, and 0 otherwise.

## The `lade status --json` contract

"`lade status --json` keeps `version`, `global_config`, `hooks`, `project_config`, and `ok`; the `hooks` object is `preexec` plus `pretool`. `age_plugin` is extra (`present`, `path`). `mise`, `log`, and `hub` are extra. Spoken stderr, README, and clap say pre-exec / pre-tool. JSON stays `preexec` / `pretool`. `--global` and JSON `global` stay" (`AGENTS.md`, House rules). Scripts and harnesses parse this JSON, so a renamed key breaks them without an error. The 0.18.0 reshape of `hooks` was announced as breaking (CHANGELOG 0.18.0, `lade status --json` hooks object), and `hooks.preexec` is `null` when no shell started Lade (CHANGELOG 0.20.0, `lade status` layout). An extra may grow or change its fields; a kept key may not. Each harness's object under `pretool` is [khcwz2ec3.d](../harness/install.md#khcwz2ec3), and `version`'s fields are [rbzjhj8tk.e](../hub/notices.md#rbzjhj8tk).

**§ <a id="fmn2jmqxb">fmn2jmqxb</a> `lade status --json` keeps its top-level keys `version`, `global_config`, `hooks`, `project_config` and `ok` across minor versions, and any other key is an extra.**

- (a) The report carries `version`, `global_config`, `hooks`, `project_config` and `ok` under those names.

- (b) `hooks` holds `preexec` and `pretool`, and `preexec` is `null` when no shell started Lade.

- (c) A harness's hook in its home config is reported under `global`, whatever the human output calls it.

## `lade bench`

`lade bench` "times the incompressible path (parse all `lade.yml` files and regex match) and the variable path (secret hydrate per loaded rule). ... Hydrates run concurrently. `--timeout` caps each rule. ... Secret values are not printed. Network acquire is not run" (CHANGELOG 0.18.0, `lade bench`). It is a diagnostic a user runs to find the slow provider, so it must be safe to paste: no value in its output, and no tunnel opened on a laptop as a side effect. One slow provider must not hide the others' times. The 5 s default cap is a default with no stated reason, not a law.

**§ <a id="w8e03rv88">w8e03rv88</a> `lade bench` times parse, match and each rule's secret hydrate.**

- (a) Each rule hydrates on its own, capped by `--timeout`, and a rule that fails or passes the cap reports a short error without stopping the others.

- (b) A rule's report names the rule's file, pattern and `when`, and holds its time, its providers and its error, and those alone.

- (c) Bench hydrates secrets only.
