import { spawnSync } from "node:child_process";

const lade = process.env.LADE_BIN ?? "lade";

// OpenCode loads every exported function in this file and calls it for hooks.
export const LadePretool = async () => ({
  "tool.execute.before": async (input, output) => {
    const command = output.args?.command;
    if (input.tool !== "bash" || typeof command !== "string") {
      return;
    }
    const payload = JSON.stringify({
      hook_event_name: "PreToolUse",
      tool_name: "Bash",
      tool_input: { command },
      hook_source: "opencode-plugin",
    });
    const result = spawnSync(lade, ["hook"], {
      input: payload,
      encoding: "utf8",
      env: { ...process.env, OPENCODE: "1" },
    });
    if (result.status !== 0 || !result.stdout?.trim()) {
      return;
    }
    let parsed;
    try {
      parsed = JSON.parse(result.stdout);
    } catch {
      return;
    }
    const updated = parsed?.hookSpecificOutput?.updatedInput?.command;
    if (typeof updated === "string") {
      output.args.command = updated;
    }
  },
});
