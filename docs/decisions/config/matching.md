# Which rules apply

A walk can load many rules, and a command must get exactly the ones meant for it: a rule that matches too much hands a secret to the wrong command, one that matches too little runs the command without its access. This note binds when a rule applies, to which audience, and what its `.` block may hold.

## Matching

The README "Patterns" section: Lade injects access only when the command matches `lade.yaml`. A key is a regular expression, searched in the command line: `"deploy .*"` and `"^tofu"` both work as written, and `.` matches every non-empty command (`test_collect_dot_matches_any_non_empty_command`). CHANGELOG 0.18.0 added lookaround (`fancy-regex`), so `(?!.*--help)` can keep a rule off `--help` (`lookahead_excludes_help_flag`).

A lookaround pattern can backtrack without end, and a hook must not hang a command line on it, so its evaluation is bounded (`src/config/patterns.rs`). A match cut off by that bound is not an answer: reading it as no match would run the command without the access its rule promised, with no word to the user. It fails the command instead. In the pre-tool hook it does not fail the hook, which a harness would answer by running the line bare: the hook hands the line to the wrap, which fails it, [t7kcndtra](../promises.md#t7kcndtra). `fancy-regex` also takes backreferences, but no source promises them.

**§ <a id="jkvdvpp5s">jkvdvpp5s</a> A rule applies to a command when its key, a regular expression, matches anywhere in the command line.**

- (a) A key anchors only where it says so, and `.` matches every non-empty command.

- (b) A key may use lookaround. A match cut off by its evaluation bound fails the command.

- (c) A key that is not a valid regular expression fails the load.

## Audience

The README "When, users, approval" section: `when` is `always` (default), `human`, or `agent`, and one pattern may hold a list of bodies with different `when` (CHANGELOG 0.18.0). A human and a harness can get a different `SSH_AUTH_SOCK` from the same rule. A body for the other audience must not touch this one: a harness-only `~` in a subfolder must not cancel a human's parent value. How Lade tells a human from a harness is the harness notes' decision.

**§ <a id="v03g22hz0">v03g22hz0</a> A rule body applies to an audience by its `when`: `always` by default, `human`, or `agent`.**

- (a) Only a body whose `when` admits this invocation's audience takes part in it, the overlay included.

- (b) One pattern may hold a list of bodies, each with its own `when`. A `when` outside the three values fails the load.

An empty list of bodies also fails the load today, but no source gives a reason for it, so it is no law.

## The `.` block

A rule's `.` block holds its configuration: `file`, `1password_service_account`, `disclaimer`, `when`, `silent`, `log` and `ttl` (`docs/architecture.md`, Dependency graph; `README.md`, When, users, approval). Each of them changes what a command gets or who must approve it, so a misspelled key is a silent drop: `disclamer:` would load cleanly and run the command without the human approval its author wrote. Every key the block does not take fails the load instead. An older Lade now fails on a key a newer one added, instead of ignoring it; a file can also pin the Lade it needs, [bvr6wk206](discovery.md#bvr6wk206), so the refusal names `lade upgrade`.

**§ <a id="t4c9tbype">t4c9tbype</a> A rule's `.` block holds only the keys Lade reads: any other key fails the load, with an error that names it and the keys the block takes.**
