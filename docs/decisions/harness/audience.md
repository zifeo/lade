# Harness integration and audience

A harness, Cursor, Claude Code, Codex or OpenCode, runs shell commands as tool calls. Lade cannot start those commands itself: it sees them only through the pre-tool hook the harness calls before a tool runs (`lade hook`), and an MCP server only through the bridge it starts (`lade mcp`). The hook answers with a rewritten command, or with nothing, and the harness does the rest. That is how the same `lade.yaml` reaches a harness and a human, [g6sqmp8tb](../promises.md#g6sqmp8tb).

The problem is that the harness owns every piece Lade depends on. It owns the reply format: a rewrite in another harness's envelope is ignored, and the command runs without Lade. It owns the working directory and the shell: a hook that reads its own directory loads the wrong yaml, and a wrapped `cd` moves Lade's child instead of the harness shell. It owns its config files: a hook file holds the user's other hooks, and a team may commit one as a requirement. And it owns the MCP protocol: one stray byte on the bridge's stdout breaks the session. These notes bind who Lade thinks it serves, here, what the hook answers and rewrites, [pre-tool](pretool.md), where the hooks live and how they stay current, [install](install.md), and what the MCP bridge passes through, [MCP](mcp.md).

The rule matching and resolution the hook hands to the wrap are the configuration note's, the ticket that carries a matched command to the wrap and its cancellation are the wrap note's, and the list of supported harnesses is [mqnfssecp](../platforms.md#mqnfssecp).

## Who runs the command

A rule may hold a body for `when: human` and another for `when: agent`, [v03g22hz0](../config/matching.md#v03g22hz0). Something has to say which one this invocation is, and it must say it once: a hook that thought "harness" and a wrap that thought "human" would plan one access and resolve another. `docs/architecture.md` section 4: "Every invocation runs `audience::detect`. That single function returns Via, Audience, and UiMode." CHANGELOG 0.18.0 (`.when` audience detection) made `--pretool` win, then the subcommand, then env signals, and stopped reading a leftover `LADE_VIA`: an inherited variable is not who runs this process.

The entry path is the strong signal. The pre-tool hook only runs for a harness, pre-exec only for a human typing in a shell, and `lade mcp` is a server a harness starts (`tests/mcp.rs`, `test_mcp_agent_when_uses_env_signal`). Only a direct `lade --`, `lade inject` or `lade eval` needs the environment. `docs/env.md`, Harness detection, is the list of signals, first match wins for the harness name the diary stores. `CURSOR_VERSION` is not on it: Cursor sets it in human terminals too, and it selected agent rules for a human (CHANGELOG 0.18.0, `.when` ignores `CURSOR_VERSION`).

A harness cannot answer a question, and one that holds a terminal is still not a person. The UI is an output of the same function: interactive only for a human with stdin and stderr at a terminal (`docs/architecture.md` section 4). The disclaimer gate that reads it is [betd87z6r](../wrap/disclaimers.md#betd87z6r). `lade add` and `lade remove` ask through the same answer, and a harness that calls them without a flag gets the error that names it.

A harness reads stderr as context: "Every message is one line starting with `lade:`, with no box, colour, or keyboard advice", and "an agent never gets progress lines" (`docs/cli.md`, What an agent reads, and Loader marks). A message is a warning, an error or a notice. A command's own result, such as the lines `lade add` prints after it wrote a binding, keeps its lines, without colour. A command line clap cannot parse is classified from what clap still reads of it, `--pretool` and the subcommand, then the harness signals, so a harness that passed a wrong flag reads its error as one line too; the classification also comes before the cache root check, whose failure a harness reads the same way.

**§ <a id="kgqztjzym">kgqztjzym</a> Each Lade invocation is classified once as a human's or a harness's, and every rule, prompt and message reads that one answer.**

- (a) A pre-tool hook, a pre-tool wrap and `lade mcp` run for a harness, and pre-exec `set` and `unset` run for a human. `--pretool` wins over the subcommand.

- (b) Otherwise only the harness signals `docs/env.md` lists decide: one of them makes it a harness's invocation, and with none it is a human's.

- (c) Lade asks a disclaimer prompt or the harness choice of `lade setup` only of a human with stdin and stderr at a terminal, and the run-anyway question only of a human with stderr at a terminal, reading its answer from the terminal Lade opens itself.

- (d) A harness reads each warning, error and notice on stderr as one plain `lade:` line, with colour and progress off.
