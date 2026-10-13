import { test } from "node:test";
import assert from "node:assert/strict";
import { nextRowTop, scrollEdges } from "../src/lib/scrollHint.ts";

test("at the top with more below: no top fade, the deep bottom fade and the hint", () => {
  assert.deepEqual(scrollEdges(0, 1000, 400), { top: 0, bottom: 72, below: 600, hint: 1 });
});

test("scrolling in grows the top fade up to its full height", () => {
  assert.equal(scrollEdges(6, 1000, 400).top, 6);
  assert.equal(scrollEdges(300, 1000, 400).top, 16);
});

test("near the end the hint fades and the bottom fade eases back", () => {
  const e = scrollEdges(552, 1000, 400);
  assert.equal(e.below, 48);
  assert.equal(e.hint, 0.5);
  assert.equal(e.bottom, 72);
  assert.equal(scrollEdges(580, 1000, 400).bottom, 52);
});

test("at the end, or with nothing to scroll: no hint, the padding's fade only", () => {
  assert.deepEqual(scrollEdges(600, 1000, 400), { top: 16, bottom: 32, below: 0, hint: 0 });
  assert.deepEqual(scrollEdges(0, 300, 400), { top: 0, bottom: 32, below: 0, hint: 0 });
});

test("overscroll and fractional sizes stay in range", () => {
  assert.equal(scrollEdges(-20, 1000, 400).top, 0);
  assert.equal(scrollEdges(600.4, 1000, 400).below, 0);
});

test("a sliver left below counts as the end: no hint", () => {
  assert.equal(scrollEdges(594, 1000, 400).hint, 0);
  assert.ok(scrollEdges(590, 1000, 400).hint > 0);
});

test("the next row is the first starting below the top", () => {
  // Two cards a row, rows 184px apart, 16px top padding.
  const tops = [16, 16, 200, 200, 384, 384];
  assert.equal(nextRowTop(tops, 0, 16, 600), 184);
  assert.equal(nextRowTop(tops, 184, 16, 600), 368);
  // Part way into a row: on to the next one.
  assert.equal(nextRowTop(tops, 100, 16, 600), 184);
  assert.equal(nextRowTop(tops, 368, 16, 600), null);
  assert.equal(nextRowTop([], 0, 16, 600), null);
});

test("a row that would leave a sliver below scrolls to the end", () => {
  assert.equal(nextRowTop([16, 200], 0, 16, 196), 196);
  assert.equal(nextRowTop([16, 200], 0, 16, 150), 150);
  assert.equal(nextRowTop([16, 200], 0, 16, 240), 184);
});

test("rows are found in any order", () => {
  assert.equal(nextRowTop([384, 16, 200], 0, 16, 600), 184);
});
