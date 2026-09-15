# Hook host (separate project)

Design note for a standalone harness adapter. Not a Lade feature. Lade
(AID), Destructive Command Guard, Orca, or anything else that today
installs its own `PreToolUse` line is a **client** of this tool.

Sources checked against the live contracts (September 2026):

- Cursor hooks: https://cursor.com/docs/agent/hooks
- Claude Code hooks: https://code.claude.com/docs/en/hooks
- Codex hooks + `codex-rs/hooks/src/schema.rs`
  (`deny_unknown_fields` on PreToolUse output)
- DCG `src/hook.rs` (HookInput, `detect_protocol`, Hermes / Antigravity
  / Agent Host)
- This repo’s current adapters: `src/pretool/{platform,response}.rs`,
  snapshots under `.cursor/`, `.claude/`, `.codex/`, `.opencode/`

## 1. Problem

Every agent already has a process hook: JSON on stdin, JSON on stdout,
an argv to spawn. The wire is not one schema. The install path is not
one file. Each product therefore:

1. Reimplements detect / extract-command / format-allow-deny-rewrite.
2. Writes a product-shaped snippet into `~/.cursor/hooks.json`,
   `~/.claude/settings.json`, `~/.codex/hooks.json`, an OpenCode JS
   plugin, `~/.grok/hooks/`, `~/.gemini/config/hooks.json`, …

Skills / APM copy trees of markdown and scripts. That is a different
job. This project does not replace it. When an agent can spawn a
binary, the install is a **pointer**, not a copy of the product.

## 2. Non-goals

- Secret hydration, bins, tunnels, `lade.yaml`, mise lock, teardown of
  a repo frontend.
- DCG packs, allow-once, AST matching.
- A new package format for skills (APM).
- Vendoring DCG or Lade source into the host.

The host owns the harness dialect and the registry of clients. Clients
own policy.

## 3. Can the APIs actually combine?

Yes for **shell pre-tool**, with a mux. No if you only register two
native hooks and hope the agent chains `updatedInput`.

### 3.1 Native multi-hook is not a pipeline

Claude documents: several `PreToolUse` handlers, then the **most
restrictive** decision (`deny` > `ask` > `allow` > `defer`). It does
**not** document that hook 2 sees hook 1’s `updatedInput`.

Cursor takes an array of commands per event. Same gap: each process
gets the original stdin.

Consequence:

| Setup | What DCG sees | What a rewriter sees | Result |
| --- | --- | --- | --- |
| Two native hooks, parallel | Original `git reset --hard` | Original, then emits `lade inject -- git reset --hard` | Deny still wins if DCG matches the original. OK for pure deny. |
| Rewriter changes the **inner** command to something DCG would block | Original only | New inner command | DCG never runs on the final string. Fail open on the rewrite. |
| Rewriter wraps without changing the inner argv | Original | Wrapped | Deny on original still works. |

A host that **sequences** clients is strictly stronger than two
entries in `hooks.json`:

1. Run every client with capability `rewrite`, left to right, feeding
   the command forward.
2. Run every client with capability `policy` on that final command.
3. First `deny` / `ask` wins. Encode once, in the harness dialect.

That sequencing is the reason this is a process, not “document that
people should install two hooks.”

### 3.2 Stdin: one envelope, many layouts

Command extraction is a table, not a guess. Observed fields:

| Harness | Event name(s) | Tool id | Command path |
| --- | --- | --- | --- |
| Cursor | `preToolUse`, also `beforeShellExecution` | `Shell` | `tool_input.command` |
| Claude Code | `PreToolUse` | `Bash` | `tool_input.command` |
| Codex | `PreToolUse` | `Bash` | `tool_input.command`; **`turn_id` required** on input |
| OpenCode | plugin `tool.execute.before` | `bash` | plugin `output.args.command`; host stdin is a shim `{command, session_id}` |
| Copilot CLI | `pre-tool-use` / `runTerminalCommand` | varies | `tool_input` or `tool_args` (object **or JSON string**) |
| VS Code Agent Host | batched | `powershell` / shell names | `toolCalls[].args` as a **JSON string**; DCG issue #252 |
| Gemini | `BeforeTool` | | snake/camel mix (`hookEventName`, `toolName`) |
| Hermes | `pre_tool_call` | `terminal` | `tool_input.command` |
| Grok | `pre_tool_use` | `run_terminal_cmd` | Claude-compat layer + `~/.grok/hooks/*.json` |
| Antigravity (`agy`) | PreToolUse-shaped | `run_command` | `toolCall.args.CommandLine` (nested) |

Detection order matters. `CURSOR_VERSION` is set in **other** agents’
terminals (this repo already special-cases it). Codex vs Claude: same
`PreToolUse` spelling; disambiguate with `turn_id` / `CODEX_*`, not
`model`. Agent Host: only treat `toolCalls` as that protocol when a
**shell** entry is in the batch. Otherwise you emit a Claude deny that
Gemini/Hermes/Codex drop (DCG comment in `detect_protocol`).

Canonical internal event (host only, never sent to the agent):

```text
Harness, Event, Tool, Command, Cwd, Session, RawValue
```

`RawValue` stays. Rewrites must round-trip unknown keys (`tool_use_id`,
`model_params`, MCP names). You cannot rebuild stdin from the canonical
struct alone.

### 3.3 Stdout and exit codes: not interchangeable

| Harness | Allow (no rewrite) | Rewrite | Deny | Exit |
| --- | --- | --- | --- | --- |
| Cursor | `{"permission":"allow"}` | `permission` + `updated_input` (snake) | `permission: deny` + `user_message` / `agent_message`; exit **2** ≡ deny | other non-0 = **fail open** |
| Claude | empty stdout, exit 0 = defer | `hookSpecificOutput.permissionDecision=allow` + `updatedInput` (camel) | `permissionDecision: deny` + reason; exit **2** also blocks | |
| Codex | same Claude shape | `updatedInput` | `permissionDecision: deny` **and non-empty reason** | schema is **`deny_unknown_fields` / `additionalProperties: false`**. Extra keys (`allowOnceCode`, `ruleId`, …) discard the **whole** reply (Codex issue discussion on 0.147: status `Failed`, call proceeds) |
| OpenCode | empty / no stdout = no-op | `{"command":"..."}` from the JS shim | **no first-class deny** in the current Lade plugin; `spawnSync` status ≠ 0 is ignored | fail open |
| Hermes | | | stdout `{"decision":"block","reason":...}` (also `action`/`message`); **non-zero exit is logged and does not abort** | must exit **0** |
| Antigravity | | | stdout `decision: block` or `deny`; non-zero exit is **not** reliable | must exit **0** |
| Copilot / Agent Host | | | Claude-shaped deny via VS Code’s Claude-compat layer (DCG) | |

Composer rules the host must implement:

1. **Never** forward a client’s raw JSON to Codex. Strip to the Codex
   wire type. DCG’s extra deny fields are legal on Claude, fatal on
   Codex.
2. Hermes / `agy`: translate deny to their stdout document, force
   exit 0. Using Claude’s exit 2 is a silent fail-open.
3. Cursor vs Claude rewrite keys are `updated_input` vs `updatedInput`.
   One internal `updated: Value`, two encoders.
4. OpenCode deny needs a plugin change (throw / set an error field).
   Until that exists, OpenCode is rewrite-only. Document it.
5. Cursor `preToolUse` vs `beforeShellExecution`: different events,
   same process model. The host maps `Event` to the config key it
   registered. Registering only `preToolUse` misses cloud/shell paths
   that only fire `beforeShellExecution` (Cursor docs list both).

### 3.4 Install files: pointer vs copy

| Harness | Config | What we write | Copy? |
| --- | --- | --- | --- |
| Cursor | `.cursor/hooks.json` (`version: 1`, `hooks.preToolUse[]`) | `{command, matcher}` | pointer |
| Claude | `.claude/settings.json` `hooks.PreToolUse[]` | `{matcher, hooks:[{type:command, command}]}` | pointer |
| Codex | `.codex/hooks.json` (or `$CODEX_HOME`) same Claude nesting | same | pointer |
| Grok | `~/.grok/hooks/*.json` | command entry | pointer |
| Gemini / agy | `~/.gemini/config/hooks.json` | command entry | pointer |
| OpenCode | `~/.config/opencode/plugins/*.js` | **JavaScript module** that `spawnSync`s the host | **copy** (thin shim) |
| Skills / APM | `.cursor/skills/…`, agent markdown | body of the skill | **copy** (out of v1) |

Merge must be surgical: keep foreign hooks, upsert ours by a stable
marker (`hookhost` argv0), not “replace the file.” Lade’s
`merge.rs` already does this for four agents. Lift that, generalize
the matcher (`Shell` vs `Bash` vs `run_command`).

User vs project vs cloud: Cursor cloud agents **ignore**
`~/.cursor/hooks.json`. Project `.cursor/hooks.json` is what cloud
runs. A host that only writes home is invisible in Cursor cloud.
Claude/Codex project files are the analogous plane.

### 3.5 Client binary API (not a Rust trait across repos)

Do not link DCG or Lade. Spawn.

Host → client stdin (versioned, **this** schema, extra fields allowed
for the client, not for Codex):

```json
{
  "v": 1,
  "harness": "codex",
  "event": "pre_tool",
  "tool": "Bash",
  "command": "git reset --hard",
  "cwd": "/repo",
  "session": "…",
  "raw": { }
}
```

Client → host stdout:

```json
{ "v": 1, "decision": "allow" }
{ "v": 1, "decision": "allow", "command": "git status" }
{ "v": 1, "decision": "deny", "reason": "…", "user_message": "…", "agent_message": "…" }
{ "v": 1, "decision": "ask", "reason": "…" }
```

Exit 0. Any non-0 from a client is **fail open for that client**
(skip it), unless the client is marked `required` in the registry.
Never inherit the client’s exit code as the harness exit: Hermes would
then no-op, Cursor would fail-open on exit 1, Claude would block on
exit 2. The host maps `decision` → harness encoding **after** the
pipeline.

Registry (host config, not the agent’s file):

```toml
[[client]]
name = "dcg"
bin = "dcg"
args = ["--hookhost"]
capability = "policy"
required = false

[[client]]
name = "lade"
bin = "lade"
args = ["hook", "--client"]
capability = "rewrite"
```

Capability `rewrite` runs before `policy`. Two rewriters: stable
order from the file. Two policies: first deny/ask stops.

`dcg` today speaks harness JSON, not this schema. Either DCG grows a
`--hookhost` mode, or the host has a **legacy adapter** that feeds
DCG the original harness stdin and parses DCG’s deny JSON **per
protocol** (worse: you reimplement DCG’s encoder). Prefer DCG
emitting the canonical decision. Until then, the host can wrap:
translate canonical → Claude-shaped stdin for DCG, parse DCG stdout
with a **tolerant** decoder, then re-encode for the real harness.
That wrapper is how you ship v1 without waiting on DCG.

## 4. Architecture

```
                    agent process
                          |
                     stdin (dialect)
                          v
                 +------------------+
                 | hookhost         |
                 | detect, extract  |
                 | pipeline         |
                 | encode dialect   |
                 +--------+---------+
                          |
              rewrite*           policy*
                          |           |
                          v           v
                       client      client
                       procs       procs
```

Crates (suggested split, one repo):

1. `hook_wire` — detect, extract, encode. Golden fixtures per harness
   (captured stdin + expected stdout). No filesystem.
2. `hook_install` — merge/unmerge pointer (and the OpenCode shim).
3. `hookhost` bin — read stdin, run registry, write stdout, `enable` /
   `disable` / `status`.

Latency budget: detect+encode in the same ballpark as today’s Lade
hook parse (milliseconds). Each client is a process; keep the set
small. Do not start a daemon for v1 unless measurement says spawn is
the cost (DCG’s own target is sub-ms in-process; a mux of two spawns
is still cheap next to vault I/O, not next to a no-op allow).

## 5. Delivery plan

**Phase 0 — fixtures.** Record stdin/stdout for Cursor `preToolUse`,
Claude `PreToolUse`, Codex `PreToolUse` (including a deny with an
extra key that Codex must reject), OpenCode shim, one Agent Host
`toolCalls` batch, one Hermes `pre_tool_call`. No code that is not
covered by a fixture.

**Phase 1 — `hook_wire`.** Table-driven encode/decode. Prove:

- Codex deny with only the allowed keys.
- Cursor `updated_input` vs Claude `updatedInput`.
- Hermes deny + exit 0.
- Agent Host batch: deny if any shell entry is destructive; do not
  mis-detect a non-shell `toolCalls`.

**Phase 2 — `hook_install`.** Enable/disable for Cursor, Claude,
Codex (pointers). OpenCode shim as the copy exception. `status`
reads back the same marker.

**Phase 3 — mux + registry.** One argv in the agent file:
`hookhost run`. Clients optional. Pipeline as in §3.1.

**Phase 4 — clients.** Lade and DCG add a canonical-decision mode
(their repos). This project stays harness + registry. Grok / Gemini /
agy when a client needs them, one fixture at a time.

## 6. Risks that are already real

1. **Codex extra keys** — combining DCG’s rich deny with Codex
   without stripping = fail open. Highest severity.
2. **Hermes / agy exit codes** — Claude’s exit 2 does not port.
3. **OpenCode** — rewrite only until the plugin can deny.
4. **Cursor cloud** — home hooks never run; project file required.
5. **`CURSOR_VERSION` pollution** — detect flag `--harness` first
   (Lade already does).
6. **Two Cursor events** — `preToolUse` ≠ `beforeShellExecution`.
7. **Native hook lists vs mux** — without sequencing, a rewriter can
   bypass a policy client. Do not advertise “just install both” as
   equivalent to the host.
8. **Skills** — remain a copy layer. Out of this binary.

## 7. Verdict

The formats combine if and only if a single process owns encode and
the client pipeline. The agents will not chain rewrites for you.
Codex will not tolerate a union JSON. Hermes will not honor a union
exit code.

That process is this project. Lade is a rewrite client. DCG is a
policy client. Neither should own the harness table.
