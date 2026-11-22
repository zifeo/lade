import { spawnSync } from "node:child_process";
import { basename, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const lade = process.env.LADE_BIN ?? "lade";
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
      output.args.command = answer.command;
    }
  },
});
