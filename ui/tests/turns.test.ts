import { test } from "node:test";
import assert from "node:assert/strict";
import { foldTurns, spent, turnLabel, type Item } from "../src/lib/turns.ts";

const user = (key: string, at?: number): Item => ({ key, kind: "user", text: "hi", at });
const said = (key: string, at?: number): Item => ({ key, kind: "assistant", text: "ok", at });
const steps = (key: string, ...labels: string[]): Item => ({
  key,
  kind: "steps",
  reasoning: "",
  tools: labels.map((label, i) => ({ id: `${key}${i}`, label, running: false, ok: true })),
});

test("a turn's steps and narration fold into one line before the turn", () => {
  const items = [user("u1", 100), said("a1", 101), steps("s1", "Reading a", "Reading b"), steps("s2", "Running tests"), said("a2", 230)];
  const f = foldTurns(items, null);
  const fold = f.before.get("a1");
  assert.ok(fold);
  assert.equal(fold.steps, 3);
  assert.equal(fold.secs, 130);
  assert.deepEqual([...f.inside.keys()], ["a1", "s1", "s2"]);
  assert.equal(turnLabel(fold), "Worked 2m · 3 steps");
});

test("the reply, and words leading into an artifact, stay out of the fold", () => {
  const art: Item = { key: "x", kind: "artifact", id: "x", title: "X", artifact_kind: "code", version: 1, payload: {} };
  const f = foldTurns([user("u1"), said("a1"), steps("s1", "Writing"), art, said("a2")], null);
  assert.deepEqual([...f.inside.keys()], ["s1"]);
});

test("a turn of one step is named by it", () => {
  const f = foldTurns([user("u1", 10), steps("s1", "Reading a"), said("a1", 13)], null);
  assert.equal(turnLabel(f.before.get("s1")!), "Reading a · 3s");
});

test("a turn with no work has no line, unless it was stopped", () => {
  assert.equal(foldTurns([user("u1"), said("a1")], null).before.size, 0);
  const f = foldTurns([user("u1", 10), { key: "x", kind: "stop", at: 22 }], null);
  const fold = f.before.get("x")!;
  assert.equal(fold.work, false);
  assert.equal(turnLabel(fold), "Stopped after 12s");
});

test("the running turn shows a live line, even before its first step", () => {
  const f = foldTurns([user("u1"), said("a1"), user("u2", 50)], { steps: 2 });
  assert.ok(f.tail);
  assert.equal(f.tail, f.live);
  assert.equal(f.live.steps, 2);
  assert.equal(f.live.start, 50);
  assert.equal(turnLabel(f.live, 10), "Thinking… 10s");
  assert.equal(turnLabel(f.live, 65), "Thinking… 1m 5s");
});

test("time reads short", () => {
  assert.equal(spent(42), "42s");
  assert.equal(spent(125), "2m");
  assert.equal(spent(3900), "1h 5m");
});
