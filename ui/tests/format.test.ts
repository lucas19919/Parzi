import { test } from "node:test";
import assert from "node:assert/strict";
import { ago, basename, clip, firstLine, formatCost, formatElapsed, plainText, runWord, shortLabel } from "../src/lib/format.ts";

const NOW = 1_800_000_000_000;

test("activity lines name the file, not its path", () => {
  assert.equal(shortLabel("Editing D:\\Repos\\Parzi\\ui\\src\\lib\\Omnibar.svelte"), "Editing Omnibar.svelte");
  assert.equal(shortLabel("Reading crates/parzi-core/src/sync.rs"), "Reading sync.rs");
  assert.equal(shortLabel("Running `cargo test -p parzi-core`"), "Running cargo test -p parzi-core");
  assert.equal(shortLabel("Opening https://docs.rs/tauri"), "Opening https://docs.rs/tauri");
  assert.equal(basename("C:\\src\\app\\"), "app");
});

test("clip cuts at a word", () => {
  assert.equal(clip("Editing Omnibar.svelte", 30), "Editing Omnibar.svelte");
  assert.equal(clip("Running cargo test -p parzi-core sync", 24), "Running cargo test -p…");
  assert.equal(clip("Supercalifragilisticexpialidocious", 10), "Supercali…");
});

test("summaries read as plain text", () => {
  assert.equal(firstLine("\n## Fixed the **freeze**\nmore"), "Fixed the freeze");
  assert.equal(firstLine("- `npm test` passes"), "npm test passes");
  assert.equal(firstLine(null), "");
  assert.equal(plainText("Done.\n\n- `name_from_prompt` drops _openers_\n```ts\nlet x = 1;\n```\nBye"), "Done. name_from_prompt drops openers Bye");
});

test("elapsed, cost and age read short", () => {
  assert.equal(formatElapsed(42_000), "42s");
  assert.equal(formatElapsed(65 * 60_000), "1h 5m");
  assert.equal(formatElapsed(-5), "0s");
  assert.equal(formatCost(null), "");
  assert.equal(formatCost(0.004), "<$0.01");
  assert.equal(formatCost(0.4213), "$0.42");
  assert.equal(formatCost(123.6), "$124");
  assert.equal(ago(NOW - 30_000, NOW), "just now");
  assert.equal(ago(NOW - 2 * 3_600_000, NOW), "2h ago");
  assert.equal(runWord("interrupted"), "Cut off");
});
