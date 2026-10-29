import { test } from "node:test";
import assert from "node:assert/strict";
import { classifyPaste, nameFromProgram, parseEnv, splitLine } from "../src/lib/cmdline.ts";
import { mergeProgress, stepName } from "../src/lib/remote.ts";

test("command lines split like a shell, without expansion", () => {
  assert.deepEqual(splitLine("npx -y @scope/server  C:\\data"), ["npx", "-y", "@scope/server", "C:\\data"]);
  assert.deepEqual(splitLine(`run "C:\\My Files\\x" 'a b' ""`), ["run", "C:\\My Files\\x", "a b", ""]);
  assert.deepEqual(splitLine("   "), []);
  assert.deepEqual(splitLine("echo $HOME"), ["echo", "$HOME"]);
});

test("a paste is a link, a copied app, or a program", () => {
  assert.equal(classifyPaste(""), "");
  assert.equal(classifyPaste("  https://mcp.notion.com/mcp  "), "link");
  assert.equal(classifyPaste('{"mcpServers":{"a":{"command":"x"}}}'), "block");
  assert.equal(classifyPaste("npx -y some-server"), "program");
  assert.equal(nameFromProgram(`"C:\\Program Files\\node.exe" server.js`), "server");
  assert.equal(nameFromProgram("npx -y @scope/server"), "server");
  assert.equal(nameFromProgram(".hidden"), "hidden");
  assert.equal(nameFromProgram("..."), "app");
});

test("env lines need NAME=value", () => {
  assert.deepEqual(parseEnv("A=1\n\nB = two=2\r\n"), { A: "1", B: " two=2" });
  assert.equal(typeof parseEnv("oops"), "string");
  assert.equal(typeof parseEnv("=x"), "string");
});

test("setup progress keeps one row per step, in arrival order", () => {
  let rows = mergeProgress([], { step: "connect", state: "run", detail: "" });
  rows = mergeProgress(rows, { step: "probe", state: "run", detail: "" });
  rows = mergeProgress(rows, { step: "connect", state: "ok", detail: "key" });
  assert.deepEqual(
    rows.map((r) => `${r.step}:${r.state}`),
    ["connect:ok", "probe:run"],
  );
  assert.equal(stepName("install"), "Install Parzi");
  assert.equal(stepName("custom"), "custom");
});
