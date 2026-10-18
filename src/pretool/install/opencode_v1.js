import { spawnSync } from "node:child_process";

const lade = process.env.LADE_BIN ?? "lade";

// OpenCode loads every exported function in this file and calls it for hooks.
export const LadePretool = async () => ({
  "tool.execute.before": async (input, output) => {
    const command = output.args?.command;
    if (input.tool !== "bash" || typeof command !== "string") {
      return;
    }
    const result = spawnSync(lade, ["hook", "--harness", "opencode"], {
      input: JSON.stringify({ command, session_id: input.sessionID }),
      encoding: "utf8",
    });
    if (result.status !== 0 || !result.stdout?.trim()) {
      return;
    }
    try {
      const updated = JSON.parse(result.stdout)?.command;
      if (typeof updated === "string") {
        output.args.command = updated;
      }
    } catch {}
  },
});
