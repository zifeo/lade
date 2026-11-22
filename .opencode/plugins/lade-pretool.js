import { spawnSync } from "node:child_process";
import { basename, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const lade = process.env.LADE_BIN ?? "lade";
// Lade 0.18 to 0.19.2 overwrite any plugin whose argv array literal starts
// with the quoted verb, so the verb is a variable here.
const verb = "hook";
// OpenCode loads a repo's plugins from its `.opencode/plugins/`: there the
// plugin is the repo's requirement, so a Lade that cannot run blocks the
// tool call. The home plugin says why on stderr and runs the line.
const required =
  basename(dirname(dirname(fileURLToPath(import.meta.url)))) === ".opencode";

function fail(message) {
  if (required) {
    throw new Error(`${message}. This repo requires Lade to run shell commands.`);
  }
  process.stderr.write(`${message}\n`);
}

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
      // spawnSync captures Lade's stderr: without this, its warnings and a
      // missing `lade` would reach no one.
      if (result.error) {
        fail(`lade: cannot start ${lade}: ${result.error.message}`);
        return;
      }
      if (result.stderr) {
        process.stderr.write(result.stderr);
      }
      if (result.status !== 0) {
        fail(`lade: ${lade} failed with ${result.signal ?? `exit ${result.status}`}`);
        return;
      }
      if (!result.stdout?.trim()) {
        return;
      }
      let answer;
      try {
        answer = JSON.parse(result.stdout);
      } catch (error) {
        fail(`lade: cannot read the answer of ${lade}: ${error.message}`);
        return;
      }
      if (typeof answer?.command === "string") {
        event.input.command = answer.command;
      }
    });
  },
};
