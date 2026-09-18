# Finalize leftover gate, plants only

The child classifies. It does not see gold. It does not edit.

Useful means a real failure, or a call site that should have
used an **existing** helper for the same concern. Not useful:
taste, a new generic, a `wc -l` move, or deleting unread fields.

Repo: the path the parent gives. Read the cited files. Then one
row per plant:

| Id | Useful? | Why |
| --- | --- | --- |

`Useful?` is `yes` or `no`. One sentence. Quote `file:line` when
you name an existing helper.

## Plants

**P1.** `src/config/query.rs` `pin_wraps` returns on the first
argv0 pin or bare version. `pins_from_rules` already last-wins
including Unset.

**P2.** Extract one overlay helper shared by `sources_for_command`,
`command_package_uri`, `package_uris`, and `pins_from_rules`.

**P3.** `src/mise/pin.rs` `locked_bin` range branch walks
`argv0s` + `store::resolve_matching_version`.

**P4.** Drop `lookup::yaml_dirs` (one caller around
`yaml_files_on_walk`).

**P5.** Drop unread `Snapshot.start` / `Snapshot.home`
(`#[allow(dead_code)]`).

**P6.** `src/mise/pin.rs` `spec_for_env` rebuilds
`mise://{prefix}/{package}@{resolved}` and drops query options.

**P7.** `lock_path_in` invents `lade.lock` next to start when
`plane` is `None`. Sole caller is event lock display.

**P8.** `Snapshot.yaml_files` vs `config.rules` after a successful
`LadeFile::build`.

**P9.** `install-local.sh` still has `cargo clean`.

**P10.** Split `src/mise/tests/prepare.rs` into implied / refresh
children so `wc -l` is under 350.
