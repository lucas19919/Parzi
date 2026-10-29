import { test } from "node:test";
import assert from "node:assert/strict";
import { formatCommand, splitCommand } from "../src/lib/acpCommand.ts";

test("a plain ACP command splits into the program and its arguments", () => {
  assert.deepEqual(splitCommand("gemini --acp"), { binary: "gemini", args: ["--acp"] });
  assert.deepEqual(splitCommand("  copilot   --acp  "), { binary: "copilot", args: ["--acp"] });
  assert.deepEqual(splitCommand(""), { binary: "", args: [] });
});

test("a quoted path stays one argument", () => {
  const line = '"C:\\Program Files\\nodejs\\node.exe" "C:\\tools\\agent.js" --acp';
  assert.deepEqual(splitCommand(line), {
    binary: "C:\\Program Files\\nodejs\\node.exe",
    args: ["C:\\tools\\agent.js", "--acp"],
  });
  assert.equal(
    formatCommand("C:\\Program Files\\nodejs\\node.exe", ["C:\\tools\\agent.js", "--acp"]),
    '"C:\\Program Files\\nodejs\\node.exe" C:\\tools\\agent.js --acp',
  );
  assert.deepEqual(splitCommand(formatCommand("C:\\Program Files\\nodejs\\node.exe", ["--acp"])), {
    binary: "C:\\Program Files\\nodejs\\node.exe",
    args: ["--acp"],
  });
});

test("formatting a command round-trips through the split", () => {
  const line = formatCommand("gemini", ["--acp"]);
  assert.equal(line, "gemini --acp");
  assert.deepEqual(splitCommand(line), { binary: "gemini", args: ["--acp"] });
});
