# Installing Lade

Lade reaches a machine four ways, and each names the release asset in its own language, while an age plugin from another release than its `lade` resolves URIs another way. This note binds what `lade upgrade` replaces and what every install path installs and checks.

## `lade upgrade`

"`lade upgrade` | Install newer `lade` and `age-plugin-lade`" (`README.md`, Commands), and in the lock table: "Untouched. This is the Lade binary". The age plugin execs `lade eval`, so a plugin from another release would resolve URIs another way, and "if `age-plugin-lade` is missing beside `lade`, the next upgrade installs it even when Lade is already current" (CHANGELOG 0.19.0). The plugin comes from the release Lade now runs, and goes to the path beside it: left to its defaults, the updater would write it over the running `lade`, and, comparing it against Lade's own version, find nothing newer for a current Lade (`plugin_installs_beside_lade_from_the_release_lade_runs`). An open shell keeps the wrap it loaded, so after an update "a box says this terminal still has the previous wrap and prints the reload command" (CHANGELOG 0.20.0, `lade upgrade`); a terminal with no wrap is told to open a new one. Lade sees only the terminal it runs in, so the 0.20.0 upgrade note asks the user to reload each open terminal, and the command speaks of this one. That `lade upgrade` runs whatever the yaml's range says is [bvr6wk206.c](../config/discovery.md#bvr6wk206).

**§ <a id="tzctg3bsv">tzctg3bsv</a> `lade upgrade` replaces the Lade binary and the `age-plugin-lade` beside it, and leaves every yaml and lock as it is.**

- (a) It installs `age-plugin-lade` from the same release beside `lade` when Lade changed, and when the plugin is missing there even if Lade is current.

- (b) **Retired.**

- (c) After an update, it tells the user that this terminal still holds the previous wrap or binary, and how to load the new one.

## The install paths

Lade reaches a machine four ways: the installer (`README.md`, Getting started and CI build), the GitHub Action `setup-lade`, which runs the installer, the image `ghcr.io/zifeo/lade`, and `lade upgrade`. The release build publishes one archive per target, `lade-v<version>-<target>.tar.gz` holding `lade` and `age-plugin-lade`, and its `.sha256` beside it (`.github/workflows/release.yml`). Four copies that name the asset differently install different binaries, or none; a copy that skips the checksum installs whatever the network handed it. The installer verifies, refuses a mismatch, and tolerates a release that published no checksum, for releases older than the checksums (`tests/installer_test.sh`, "missing checksum is tolerated (backward compatibility)"). A silent drop becomes an explicit failure: the installer and `lade upgrade` install such a release only with a warning that names the missing `.sha256`, and only on a 404, since any other failure to fetch it may be a network that dropped it (installer_test.sh, "a failed checksum download aborts"), and an installer that finds a published checksum but no tool to compute one stops and names the tool to install. The image keeps refusing it, since it only builds releases that publish one. A release before 0.19.1 holds `lade` alone (CHANGELOG 0.19.1): the installer and `lade upgrade` then install `lade` with a warning that names the missing plugin, and the image, which `Dockerfile` builds by review, skips it. The Action says the same: "verify its SHA256 checksum, and add the directory to PATH" (`action.yml`), and it resolves `latest` to a version before it caches, so a cached binary never stands in for a newer release.

The installer and the Action are run by scripts and CI with no one to press Enter: "`CI` | Skip shell-wrap prompts; same as `ASSUME_YES` for the installer", and `ASSUME_YES` is "`1` / `true`" (`docs/env.md`, Installer and CI). `OUT_DIR` is the install directory (same table). A binary off `PATH` is one the user cannot run by name, so the installer puts the directory there in the user's shell profile (owner answer, 2026-11-19), the file `lade setup` writes its hook to for the login shell `SHELL` names, `~/.profile` for a shell Lade has no hook for. It writes one line of its own, marked `lade-path-do-not-edit`, beside the `eval "$(lade on)"` line `lade setup` marks `lade-do-not-edit`: Lade rewrites every line holding its marker, and the installer's marker does not hold that text, so each keeps the other's line (`tests/installer_test.sh`, section 9; `src/preexec/profile.rs`, `writing_the_hook_keeps_the_line_the_installer_adds_to_path`). A second run finds its line and writes nothing, a run for another directory replaces its line, and a profile that already sets `PATH` with the directory is the user's, so it stays as it is. The Action adds `OUT_DIR` to `PATH` through `GITHUB_PATH` and runs the installer with it on `PATH`, so a runner's profile stays as it is.

The Action and the image are not parsed for markers: `action.yml` and `Dockerfile` follow this law by review, and the image fetches the musl asset of its architecture and refuses one whose checksum is missing or wrong.

**§ <a id="tp05szjjy">tp05szjjy</a> Every install path installs the release asset of the machine's target, holding both `lade` and `age-plugin-lade`, and refuses one whose SHA256 differs from the checksum published beside it.**

- (a) The asset is `lade-v<version>-<target>.tar.gz`, where `<target>` is the target triple the release build publishes for this machine.

- (b) An asset Lade cannot check against its published `.sha256` fails the install on every path: a mismatch, a checksum fetch that fails other than with a 404, or no tool to compute the SHA256.

- (c) Both binaries go into one directory, `OUT_DIR` for the installer and the Action, and when it is not on `PATH` the installer adds it once to the user's shell profile, on a line marked `lade-path-do-not-edit`, and keeps a profile that already sets `PATH` with it as it is.

- (d) The installer proceeds straight to the install when `CI` is set to anything but empty, `0` or `false`, when `ASSUME_YES` is `1` or `true`, or when stdin is not a terminal.

- (e) Only a 404 on the `.sha256` installs, through the installer and `lade upgrade`, and only with a warning that names the missing checksum.
