# Pins

A package is the third family of a `lade.yaml` binding (`README.md`, Families): a CLI pinned for the commands a rule matches, or a setup-only package. `^tofu: { tofu: mise://aqua/opentofu/opentofu@1.8.2 }` makes `tofu` run that exact OpenTofu for that command, whatever the machine has installed, and nothing changes once the command ends (`README.md`, Package). Lade does not install CLIs itself: it asks mise, the tool manager the `mise://` scheme names, and keeps the versions in a lock the repo commits. `lade setup` wires a repo (its packages and lock, its pre-tool hooks, the shell hook the first time), `lade update` moves the lock, `lade teardown` undoes the repo's part, and `lade upgrade` replaces the Lade binary. Lade itself reaches a machine through the installer, a GitHub Action, a container image and `lade upgrade`.

The problem is that every one of these has a second source of truth waiting to win silently. A Homebrew `tofu` on `PATH` looks like the pin. A newer CLI already installed looks like the locked one. A user's `~/.config/mise/config.toml` or a project `mise.toml` changes what mise installs or which environment it exports. A repo that already uses mise has its own `mise.toml` and `mise.lock`, and a second lock beside them would drift from the first. mise itself rejects a version range on most backends by printing nothing and exiting 0 (CHANGELOG 0.19.1, mise ranges). The four install paths each name the release asset in their own language. A teammate, a harness and CI must all get the same binary for the same command, or a pin is only a suggestion.

How long the pin stays on `PATH` is [jfxr4rmae](../promises.md#jfxr4rmae), that a failed pin cancels the command on every door is [t7kcndtra](../promises.md#t7kcndtra), the platforms Lade ships for are [mqnfssecp](../platforms.md#mqnfssecp), where setup writes pre-tool hooks is [rkpfgpx72](../harness/install.md#rkpfgpx72), that `lade upgrade` runs whatever the yaml's version range says is [bvr6wk206](../config/discovery.md#bvr6wk206), and that `lade add` runs setup is [qp8tmdhae](../config/editing.md#qp8tmdhae). These notes state what a pin means, here, where its version lives and who may move it, [lock](lock.md), how Lade runs mise, [mise](mise.md), what each repo verb does, [setup](setup.md), and what each install path does, [install](install.md).

## A pin on the command's PATH

"The key is the argv0 you type. If the binary is `tofu1.8`, the key is `tofu1.8`. Lade asks the tool manager to pin it, then puts that install first on PATH for this process" (`README.md`, Package). The key names the binary, not the package: two pins of one package can expose two binaries, and the key is what a command line calls. A `.` pin applies to every matching command (CHANGELOG 0.19.0), so `which kubectl` under a `.` rule that pins `kubectl` sees the pinned one (`catch_all_kubectl_pin_applies_to_which`). A release that unpacks to a folder named like its binary (age: `age/age`) once put the wrong folder on `PATH`, so the binary must be a file (CHANGELOG 0.20.0, Pinned CLI folder). Only the key names the binary: Lade once also looked for a file named after the package, so `json: mise://aqua/jqlang/jq@1.7.1` put `jq` first on `PATH` while the command line called `json`. A key that names no file of the install now fails like any missing pin (`the_key_is_the_only_binary_name`). OpenTofu ships `tofu`, so its pin is `tofu: mise://aqua/opentofu/opentofu@1.8.2`.

A login shell is the exception. A catch-all rule that pins `zsh` or `fish` for a test suite would otherwise replace `/bin/zsh` for every command the rule matches, including the shell Lade starts the command in: "`zsh`, `fish`, and `bash` on a catch-all rule do not replace `/bin/zsh`. They inject only when the wrapped command is that shell" (CHANGELOG 0.19.0, Login shells). The code applies the same rule to `sh`, `dash` and `ksh` (`src/mise/spec.rs`, `is_user_shell`), shells no source names, so the law binds the three the source does. The pre-tool hook decides whether to wrap with the same rule, so a pinned shell alone does not cost a wrap; a shell pin that does not parse still sends the line to the wrap, which parses every pin first and fails it, as `lade --` does (`a_malformed_shell_pin_fails_the_line_as_the_wrap_does`).

A pinned tool may need its own environment, such as `RUSTUP_TOOLCHAIN` for a Rust pin (`inject_rust_pin_exports_toolchain`). That variable goes into the command with the pin. When the yaml binds the same name to another value, one of the two would be dropped without a word, so the command fails instead: "a failed secret, tunnel, pin, ticket, or env conflict cancels the command" (CHANGELOG 0.20.0).

A pin is a Lade URI so that a binding's family comes from its scheme (`README.md`, Package, scheme table). The mise CLI form `aqua:jqlang/jq@1.7.1` and a bare version `tofu: 1.8.2` look like a pin and are not; Lade names the form to write instead of reading them as a secret (`legacy_cli_in_lade_yml_is_an_error`, `inject_refuses_bare_version`). Every `mise://` value is a pin, so one with an unknown backend, or with the CLI form inside it (`mise://github:owner/repo@1.0.0`), fails rather than reach the command as a raw value (`an_unknown_backend_is_a_pin_that_fails`, `a_cli_form_inside_the_uri_fails_and_names_the_uri`). A bare version is a pin's mistake only under the key of the command it runs: under another key, `VERSION: 1.2.3` is an ordinary variable, and Lade exports it. The backends Lade reads are the names `mise backends ls` prints, checked against mise 2026.11.10, and `pipx`, which mise still reads as `pypi` (`a_backend_mise_lists_is_a_pin_backend`). A backend a later mise adds fails until Lade lists it.

**§ <a id="aq3qybz6p">aq3qybz6p</a> A `mise://` pin on a matched rule puts the pinned install of the binary its key names first on `PATH` for that command.**

- (a) The key is the name of the binary: Lade puts first on `PATH` the directory that holds an executable file of that name in the pinned install.

- (b) A pinned login shell, `bash`, `zsh` or `fish`, goes on `PATH` only for a command that is that shell.

- (c) A variable the pinned tool sets that the yaml binds to another value fails the command.

- (d) A pin is written `mise://<backend>/<package>@<version>` with a backend mise knows: any other `mise://` value, the mise CLI form, and a bare version bound to the key of the command being run fail the command.

## No stand-in for a missing pin

"A Homebrew binary on PATH is not the pin" (`README.md`, Package). If a missing pin fell back to whatever `PATH` holds, the command would run a CLI the repo never chose, at a version nobody reviewed, and the user would not know: the pin would hold on one laptop and not on the next. CHANGELOG 0.19.0 (mise pins): "A miss installs from the lock in an isolated temp dir, then refuses if the binary is still missing. Homebrew is not the pin." The install happens for the binary the command runs; any other missing pin that applies to the command, and a missing implied CLI, is a setup the repo has not done yet, and the error says so. A pinned login shell applies only to a command that is that shell ([aq3qybz6p.b](pins.md#aq3qybz6p)), so a missing one does not stop another command: "locked op is missing. Run `lade setup` to install the pinned CLI. Homebrew or another PATH binary is not used" (`implied_missing_refuses_path_fallback`, `inject_refuses_homebrew_when_install_leaves_bin_missing`).

A wrap does not fetch mise: "Setup and upgrade may fetch. Inject must not" (`src/mise/ensure.rs`; CHANGELOG 0.19.0, mise binary). A CLI already in the store still starts when mise itself is absent (CHANGELOG 0.19.0, Inject with a store hit).

**§ <a id="rtcp7pj7e">rtcp7pj7e</a> When a pinned or implied CLI is not installed at its version, the command fails.**

- (a) A missing pin of the binary the command runs is installed at its locked version, else at the version its yaml pin resolves to, and the command fails when the binary is still missing.

- (b) Any other missing pin of the matched rules that applies to the command, and a missing implied CLI, fails the command with an error that names `lade setup`.

## Implied CLIs

"Provider URIs can **imply** a locked CLI on setup (`op://` → `op`, `vault://` → `vault`, …) even when the yaml has no `mise://` pin" (`README.md`, Commands). A provider that drives its product's CLI ([cbck003hy](../secrets/providers.md#cbck003hy)) is only as reproducible as that CLI: a teammate on an older `op` gets other behaviour, or none. So the provider table that states each CLI's tested minimum version also names its mise package, and that row becomes a pin with no line in the yaml (`src/mise/implied.rs`). "`ssh://` stays the OpenSSH binary already on `PATH`. `vault://` still hydrates over HTTP and also locks the Vault CLI" (CHANGELOG 0.19.0). The setup-only schemes imply their CLI too: `apm://` locks `apm` (`apm_implies_lock`).

The yaml always wins: a pin of the same key, or of the same package under another key, replaces the implied one (`yaml_pin_wins`, `same_backend_is_not_implied_again`). Setup locks that yaml pin wherever it sits on the walk, so a wrap uses it too, even when it sits on a rule the command did not match: the implied range would ask for a version the lock does not hold (`a_yaml_pin_on_another_rule_replaces_the_implied_one`). The provider table states no highest version today, so an implied pin is open above. Two different packages that both end in `cli` are two pins (CHANGELOG 0.19.0, Pin identity). A repo that already lists the CLI in its own `mise.toml` keeps its row: "implied CLIs use the in-range row as written. A yaml pin still wins. A missing tool is added with `mise use`" (CHANGELOG 0.19.2, Project mise.toml). The README table calls this a range that "may heal from `mise.toml`", and on `lade update` "Implied and ranged pins move to the latest match" (`README.md`, lock table), so update writes the version it resolves into that row.

**§ <a id="ba4zs2twj">ba4zs2twj</a> A provider URI whose provider runs a product CLI implies a pin of that CLI, unless the yaml pins that CLI itself.**

- (a) The implied pin is the range from the lowest version Lade supports for that provider. `ssh://` implies none and uses the OpenSSH on `PATH`.

- (b) A yaml pin with the same key or the same package, on any rule of the walk, replaces the implied one.

- (c) Setup locks and installs the implied pin of every provider URI on the walk, setup packages included, as it does a yaml pin.

- (d) In a repo whose `mise.toml` lists that CLI at a version inside the implied range, setup and a wrap use that row as written, its options included, and `lade update` writes into it the version it resolves for the range.

- (e) On a machine attached to a laptop, a URI that resolves through the link implies no pin, [a36hqp4ep](../remote.md#a36hqp4ep).
