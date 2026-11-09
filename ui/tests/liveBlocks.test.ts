import { test } from "node:test";
import assert from "node:assert/strict";
import MarkdownIt from "markdown-it";
import { splitBlocks } from "../src/lib/liveBlocks.ts";

const md = new MarkdownIt();
const liveBlocks = (text: string) => splitBlocks(text, md.parse(text, {}));

test("a streaming reply splits into its top-level blocks", () => {
  const text = "# Plan\n\nFirst we look.\n\n- one\n- two\n\nThen we act.";
  assert.deepEqual(liveBlocks(text), ["# Plan\n", "First we look.\n", "- one\n- two\n", "Then we act."]);
});

test("a list or table stays one block however it grows", () => {
  const list = "Steps:\n\n1. Read\n\n   still the first item\n2. Write";
  const blocks = liveBlocks(list);
  assert.equal(blocks.length, 2, JSON.stringify(blocks));
  assert.match(blocks[1], /^1\. Read[\s\S]*2\. Write$/);
  const table = "| a | b |\n| - | - |\n| 1 | 2 |\n| 3 |";
  assert.deepEqual(liveBlocks(table), [table]);
});

test("an open code fence is one block to the end of the text", () => {
  const text = "Here:\n\n```ts\nconst a = 1;\n\nconst b = 2;";
  const blocks = liveBlocks(text);
  assert.equal(blocks.length, 2);
  assert.equal(blocks[1], "```ts\nconst a = 1;\n\nconst b = 2;");
});

test("finished blocks keep the same text as the reply grows", () => {
  const a = liveBlocks("One.\n\nTwo is gro");
  const b = liveBlocks("One.\n\nTwo is growing still.\n\nThree");
  assert.equal(a[0], b[0]);
  assert.equal(b.length, 3);
  assert.deepEqual(liveBlocks(""), []);
  assert.deepEqual(liveBlocks("   \n"), []);
});
