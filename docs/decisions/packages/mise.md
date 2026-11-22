# Running mise

Lade installs CLIs through mise, and mise reads configuration from the user's home, XDG, the system and every directory up the tree, any of which can change what it installs. This note binds that Lade's mise runs follow only Lade's pin config, and which mise binary Lade runs.

## mise runs isolated

A pin must mean the same CLI on every machine. mise reads configs from the user's home, XDG, the system, and every directory up the tree, and any of them can change what it installs or which environment it exports: a home `JAVA_HOME` leaked into a Rust command (`inject_stale_sidecar_does_not_export_java_home`, `set_mise_ls_ignores_home_java_keeps_project_node`), and a `rust-toolchain` file overrode the pin (`rust_toolchain_file_does_not_override_pin`). So every mise run Lade starts, the queries that list versions included, reads a pin-only config from Lade's cache or none (the version queries and setup's `mise use` read none), drops the mise environment overrides, and lists every other mise config as ignored, `.tool-versions` files included (`install_from_url_isolates_pin_only_config`, `isolate_includes_home_global_and_parent_config_dir`, `a_mise_query_ignores_the_users_mise_config`). mise also picks config files through `MISE_SYSTEM_CONFIG_FILE`, `MISE_OVERRIDE_CONFIG_FILENAMES`, `MISE_DEFAULT_CONFIG_FILENAME` and their `.tool-versions` twins, checked against mise 2026.11.10, so every mise run Lade starts drops them too (`a_mise_run_drops_every_variable_that_names_a_config_file`). The `mise --version` probe that picks which mise runs reads no config.

The other direction matters as much. mise writes shims, and "the vault test ... rewrote `~/.local/share/mise/shims`, and broke every agent shell" (`AGENTS.md`, Build & test). Shims in the user's directory shadow the CLIs of every shell the user opens, so Lade's runs keep theirs in Lade's cache. A pin's environment comes from that tool alone, read through `mise env --json-extended` and cached per tool and version, and a cached entry for another tool is ignored (`take_this_tool_drops_other_tools_and_config_env`, `load_rejects_other_tool_sidecar`).

**§ <a id="fvghrwpwb">fvghrwpwb</a> A pin installs and exports what Lade's own pin config says, whatever the user's mise configuration holds, and Lade's mise runs keep their shims in Lade's cache.**

- (a) Every mise run Lade starts ignores every home, system and project mise config up the tree, and an install or an environment export reads Lade's pin config.

- (b) Every mise run Lade starts writes its shims into Lade's cache.

- (c) A pin brings into the command only the environment its own tool sets.

## Which mise

"`lade setup` uses PATH mise when it is already in range. Otherwise it installs the official binary (`$MISE_DATA_DIR/bin/mise`, else `~/.local/share/mise/bin/mise`). There is no second copy under Lade's data dir. Inject does not fetch mise" (CHANGELOG 0.19.0, mise binary). `LADE_MISE` names a mise to try before the one on `PATH` (`docs/env.md`, User overrides), and like any other it runs only inside the range. The range (`>=2024.0.0` today, `src/mise/range.rs`) is the floor Lade is tested against; an older mise would misread the lock. A second copy of mise in Lade's own directory would split the user's installs between two managers. A wrap must not stall a command on a download, and a CLI already in the store runs with no mise at all (CHANGELOG 0.19.0, Inject with a store hit). `lade update` and `lade add` run setup's pins, so they fetch as setup does. No mise run, a wrap's or a query's, falls back to a mise outside the range (`an_out_of_range_mise_is_never_run`). `LADE_MISE_FETCH=0` turns the fetch off, and setup then fails and says how to install mise (`docs/env.md`, User overrides; `setup_fails_with_install_hint_when_fetch_is_off`).

**§ <a id="kwbgdgf2x">kwbgdgf2x</a> `lade setup` runs the mise `LADE_MISE` names, else one already on `PATH`, when its version is in the range Lade supports, and otherwise fetches the official mise release into mise's own data directory.**

- (a) Lade runs only a mise inside the range. When the fetch fails or is turned off, setup fails and says how to install mise.

- (b) The fetched mise is `$MISE_DATA_DIR/bin/mise`, else `~/.local/share/mise/bin/mise`.

- (c) Only `lade setup`, `lade update`, `lade add` and `lade upgrade` fetch mise, and a wrapped command that needs mise and finds none in range fails.
