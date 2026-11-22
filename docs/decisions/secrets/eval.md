# `lade eval` and the age plugin

Scripts and age need one value outside any rule: a script reads a URI, and age uses a Lade URI as a key. This note binds what `lade eval` resolves and prints, and how `age-plugin-lade` reaches it.

## One URI at a time

`lade eval <uri>` resolves one URI for debugging and scripts (`README.md`, Secret and Commands; CHANGELOG, #132). It prints the value itself, since a script and `age-plugin-lade` need it. Lade does not log in for the user: a provider that is not logged in fails, and the user runs that product's own login. Without `--ttl`, eval stays out of the hub, so a one-off read leaves nothing behind (`README.md`, Secret; CHANGELOG 0.20.1).

**§ <a id="xrjs58t2q">xrjs58t2q</a> `lade eval <uri>` resolves one URI through the providers and prints its value on stdout.**

- (a) The value is printed as resolved, unmasked.

- (b) Without `--ttl`, eval resolves the URI through its provider alone.

- (c) A tunnel or package URI fails, with or without `--ttl`, before eval resolves or records it.

`lade eval <uri>` is "Resolve one secret URI to stdout" (`README.md`, Commands). A tunnel or package URI is no secret: no secret provider claims it, so without (c) plain eval printed the URI's own text as its value and wrote an `access` row, while `--ttl` refused the same URI, [y8w7xhrr1.b](../cache/ttl.md#y8w7xhrr1). Copies that disagree align, and a silent wrong value becomes an error (test `eval_of_a_tunnel_or_package_uri_fails_without_ttl`).

age and rage can use a key held in a vault through `age-plugin-lade` (`README.md`, age plugin). The plugin does not resolve anything itself: `lade eval` does, so age and rage resolve a Lade URI the same way Lade does (CHANGELOG, #203). A resolved value that names a plugin identity would make age run that plugin, and an `AGE-PLUGIN-LADE` one would make the plugin call itself (CHANGELOG 0.20.0, "age plugin identities"). A post-quantum key must not share a file with a classic one, or the classic stanza undoes it (CHANGELOG 0.20.0, "age plugin labels").

Post-quantum keys from age 1.3 (`age1pq1`, `AGE-SECRET-KEY-PQ-1`) go through `age-plugin-pq` on `PATH`, and a missing one fails with an error that names it. That bridge goes away once rage supports these keys (CHANGELOG 0.20.0), so it is a current limit, not a law.

**§ <a id="mcf6rtczf">mcf6rtczf</a> `age-plugin-lade` lets age use a Lade URI as a key, resolved by `lade eval`.**

- (a) The plugin resolves each URI by running `lade eval`, and age gets the value it prints.

- (b) The plugin runs the `lade` a set `LADE_BIN` names, else the one next to itself, else the one on `PATH`. A set `LADE_BIN` that is not a file fails.

An explicit value wins: whoever sets `LADE_BIN` names the `lade` to run, as tests that pin the built binary do (`crates/age-plugin-lade/tests/pq.rs`, `tests/shell.rs`), and a `LADE_BIN` that names no file is a mistake to report, not a reason to run another `lade` (owner answer, 2026-11-19; upstream test `lade_bin_env_wins`). The OpenCode plugin reads `LADE_BIN` the same way, ahead of the `lade` it was written for (test `both_plugins_run_lade_bin_over_path`), and says on stderr when it cannot start the one `LADE_BIN` names (`both_plugins_fail_on_a_lade_bin_that_is_not_a_file`). Tests: `lade_bin_env_wins`, `lade_bin_env_wins_over_path`, `lade_bin_env_missing_file_errors`, `path_finds_lade_when_no_env`.

- (c) A resolved value with an `AGE-PLUGIN-` line anywhere in it is refused.

- (d) Wrapping one file to a post-quantum key and a classic key together fails.
