# How a binding gets its value

Once a rule applies, each of its bindings must take one value, the same on every door. A value can differ per teammate, be built from other bindings, or be a step that must never reach the process. This note binds how a binding picks and builds its value, and which names may reach the environment.

## Per-user values

The README "When, users, approval" section and CHANGELOG 0.13.0: a binding may map user names to values, with `.` as the default, and `lade user` sets the name. Teammates share one `lade.yaml` and each reaches their own token. CHANGELOG 0.20.0 made the OS user the fallback when `config.json` names none. A map with no entry for this user and no `.` gives this user nothing from that body (`test_resolve_lade_secret_user_no_match_no_default`, `user_keyed_secret_skips_wrap_when_os_user_misses`), and the parent's value, the default, still applies. The README rule that YAML `~` on a key cancels a parent value holds inside a map too: `alice: ~` cancels the parent's value for alice alone.

The user decides which secret a command gets, so every path must resolve the same user. A `config.json` that cannot be read is a failure: CHANGELOG 0.20.0 lists a user failure among those that cancel the command. Falling back to the OS user on one path and failing on another would let a pre-tool hook plan with one user and the wrap hydrate with another. The hook does not fail on such a file, which a harness would answer by running the line bare: it hands the line to the wrap, which fails it. An empty `$USER` names no one, so it counts as unset.

**§ <a id="dcrxp986s">dcrxp986s</a> A binding may map Lade users to values, and it takes the value of the Lade user, else the `.` entry, else none from that body.**

- (a) When a map has no value for this user and no `.`, an earlier body's value for this user still applies.

- (b) The Lade user is `user` in `config.json`, else a non-empty `$USER`, else a non-empty `$USERNAME`. A `config.json` that cannot be read fails the command on every path that resolves the user.

- (c) `lade user NAME` saves the user, `lade user --reset` drops it, and `lade user` alone prints it. An empty name fails.

- (d) A `~` as the value for this user, or as the `.` entry this user falls back to, cancels an earlier body's value for this user, as a `~` on the key does.

## Composition

`docs/architecture.md` section 5: every matching command builds a dependency graph from its bindings before any provider is invoked, `${NAME}` references another binding, and the graph and its values are scoped to one invocation. A typo in a reference would otherwise send a half-built value, such as a `Bearer` header with no token, to the command, and a cycle would never finish (`dag_rejects_missing_dependency`, `dag_rejects_cycles`). A bare `$NAME` that names no binding is left as text, since a shell provider owns that syntax (`dag_allows_unknown_bare_shell_variables`).

The graph spans every binding the invocation applies, across files, as section 5 says; the README phrase "in the same rule" describes the common case. A binding served from the hub is still a binding of the invocation, so a reference to it resolves like a reference to a fetched one. Only secrets are nodes: a tunnel, a pin or a package feeds no reference, so a `${NAME}` that names one fails as a missing dependency.

Lade checks the graph when it plans the command, before it fetches a rule's 1Password service-account token or starts a tunnel, so a broken reference costs no provider call.

Shell providers keep their script text: section 5 says direct dependencies are injected into that shell's environment, which preserves shell syntax such as command substitution and pipes and keeps secret values out of the script text (`test_collect_hydrate_injects_dependencies_into_shell_provider`).

**§ <a id="v3rjmav3z">v3rjmav3z</a> A binding may build its value from other bindings of the same invocation with `${NAME}`, and each reference resolves before the binding that uses it.**

- (a) The references form one graph over every secret binding the invocation applies, across files, and a binding served from the hub resolves a reference like a fetched one.

- (b) A `${NAME}` that names no binding, or a cycle, fails the command before any provider runs.

- (c) A shell provider (`sh://`, `bash://`, `zsh://`, `fish://`) receives its dependencies as environment variables, and its script text keeps each reference as a shell variable for the shell to expand.

## Intermediate bindings

The README "Hierarchy" section and `docs/architecture.md` section 5: `.NAME` is intermediate, used to build another value and not exported. It never reaches the child environment, a temporary file, or an MCP header (CHANGELOG 0.17.0). This is how a raw token becomes `Bearer ${TOKEN}` without the raw token sitting in the process environment, where any child could read it (`test_collect_hydrate_interpolates_private_binding_without_emitting_it`). A public binding and `.NAME` cannot coexist, or the overlay would decide by accident whether a secret is exported (`test_collect_hydrate_rejects_public_private_collision`). That holds whatever the family of the public one: a public tunnel `NAME` beside a `.NAME` secret would export a port under the name the reference reads as a secret.

Only a secret can feed a reference. A `.NAME` that holds a tunnel or a package builds nothing and was dropped without a word, so it fails instead.

**§ <a id="dyat01nyj">dyat01nyj</a> A `.NAME` binding resolves for the bindings that reference it alone, and Lade keeps its value in its own memory.**

- (a) A public `NAME` and a `.NAME` that apply to the same invocation fail the command.

- (b) A `.NAME` is a valid environment variable name and holds a secret. One that holds a tunnel or a package fails the command.

## Binding names

CHANGELOG 0.20.0 "Binding names": a binding exported to the env must be a valid env var name, letters, digits and `_`, not starting with a digit. A name like `X;touch f` was pasted into the shell code the pre-exec hook evals, so a `lade.yaml` key could run a command in the user's shell. A binding written to a file keeps any field name, since it never reaches shell code.

**§ <a id="ps6s9115k">ps6s9115k</a> A binding exported to the environment has a valid environment variable name, and any other name fails the command.**

- (a) A binding written to a file by its rule's `file:` may use any field name.
