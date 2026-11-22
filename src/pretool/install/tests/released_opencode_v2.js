import { spawnSync } from "node:child_process";

const lade = process.env.LADE_BIN ?? "lade";
// Lade 0.18 to 0.19.2 overwrite any plugin whose argv array literal starts
// with the quoted verb, so the verb is a variable here.
const verb = "hook";

// OpenCode v2 plugin API: default export with id + setup; hooks are
// registered on ctx. OpenCode v2 renamed the bash tool to "shell", so both
// names are matched.
export default {
  id: "lade-pretool",
  async setup(ctx) {
    await ctx.tool.hook("execute.before", (event) => {
      const command = event.input?.command;
      if (
        (event.tool !== "bash" && event.tool !== "shell") ||
        typeof command !== "string"
      ) {
        return;
      }
      const result = spawnSync(lade, [verb, "--harness", "opencode"], {
        input: JSON.stringify({ command, session_id: event.sessionID }),
        encoding: "utf8",
      });
      if (result.status !== 0 || !result.stdout?.trim()) {
        return;
      }
      try {
        const updated = JSON.parse(result.stdout)?.command;
        if (typeof updated === "string") {
          event.input.command = updated;
        }
      } catch {}
    });
  },
};
