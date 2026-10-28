import { test } from "node:test";
import assert from "node:assert/strict";
import { splitSegments } from "../src/lib/segments.ts";

test("closed fences split into typed segments", () => {
  const segs = splitSegments('Hello\n```parzi-artifact\n{"id": "a", "content": "x"}\n```\nBye', false);
  assert.deepEqual(segs.map((s) => s.kind), ["md", "artifact", "md"]);
  assert.equal(segs[0].kind === "md" && segs[0].body, "Hello\n");
});

test("a trailing unclosed fence still becomes its card", () => {
  const segs = splitSegments('Here it is:\n```parzi-artifact\n{"id": "a", "content": "x"}\n', false);
  assert.deepEqual(segs.map((s) => s.kind), ["md", "artifact"]);
  const card = segs[1];
  assert.ok(card.kind === "artifact" && (card.body as { id: string }).id === "a");
});

test("a trailing unclosed viz block becomes a viz segment", () => {
  const segs = splitSegments('```viz\n{"type": "bars", "data": [', false);
  assert.deepEqual(segs.map((s) => s.kind), ["viz"]);
});

test("unclosed but invalid JSON stays raw text", () => {
  const segs = splitSegments('Note:\n```parzi-artifact\nnot json at all', false);
  assert.deepEqual(segs.map((s) => s.kind), ["md"]);
});

test("plain prose mentioning fences is untouched", () => {
  const segs = splitSegments("write viz blocks like this", false);
  assert.deepEqual(segs.map((s) => s.kind), ["md"]);
});
