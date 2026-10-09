import { test } from "node:test";
import assert from "node:assert/strict";
import { byDay, formatTime } from "../src/lib/time.ts";

test("formatTime handles empty and epoch", () => {
  assert.equal(formatTime(""), "");
  assert.equal(formatTime("not-a-date"), "");
  assert.match(formatTime(0), /\d/);
});

test("byDay groups newest first", () => {
  const now = Date.now();
  const days = byDay([
    { at: now, v: 1 },
    { at: now, v: 2 },
    { at: now - 3 * 86_400_000, v: 3 },
  ]);
  assert.equal(days.length, 2);
  assert.equal(days[0].items.length, 2);
  assert.equal(days[1].items.length, 1);
});
