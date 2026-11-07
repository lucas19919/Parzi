import { test } from "node:test";
import assert from "node:assert/strict";
import { parseViz, type VizSpec } from "../src/lib/viz/spec.ts";
import { countText, lerpNumbers, numbersOf, shapeOf, withNumbers } from "../src/lib/viz/tween.ts";

const spec = (v: unknown): VizSpec => {
  const p = parseViz(v);
  assert.ok(p.ok, JSON.stringify(p));
  return (p as { ok: true; spec: VizSpec }).spec;
};

test("numbers go back where they came from; progress counts up, bars keep their scale", () => {
  const s = spec({ type: "bars", data: [{ label: "a", value: 4 }, { label: "b", value: 10 }] });
  assert.deepEqual(numbersOf(s), [4, 10]);
  const half = withNumbers(s, numbersOf(s), 0.5);
  assert.deepEqual(half.type === "bars" && half.items.map((p) => p.value), [4, 10], "bars grow by drawing, not by value");
  const prog = spec({ type: "progress", data: [{ label: "x", value: 0.8 }] });
  const filling = withNumbers(prog, numbersOf(prog), 0.5);
  assert.equal(filling.type === "progress" && filling.items[0].value, 0.4);
  assert.deepEqual(withNumbers(s, [1, 2]).type === "bars" && withNumbers(s, [1, 2]), { ...s, items: [{ label: "a", value: 1 }, { label: "b", value: 2 }] });
});

test("a line keeps its gaps and only its numbers move", () => {
  const s = spec({ type: "line", data: { labels: ["a", "b", "c"], series: [{ name: "x", values: [1, null, 3] }] } });
  assert.deepEqual(numbersOf(s), [1, 3]);
  const moved = withNumbers(s, [5, 6]);
  assert.deepEqual(moved.type === "line" && moved.series[0].values, [5, null, 6]);
});

test("an update keeps its shape when only numbers change", () => {
  const a = spec({ type: "bars", data: [{ label: "a", value: 4 }] });
  const b = spec({ type: "bars", data: [{ label: "a", value: 9 }] });
  const c = spec({ type: "bars", data: [{ label: "z", value: 9 }] });
  assert.equal(shapeOf(a), shapeOf(b));
  assert.notEqual(shapeOf(a), shapeOf(c), "new labels make a new chart");
});

test("whole numbers count as whole numbers and new values start at zero", () => {
  assert.deepEqual(lerpNumbers([0, 0], [10, 2.5], 0.33), [3, 0.8]);
  assert.deepEqual(lerpNumbers([4], [4, 8], 0.5), [4, 4]);
  assert.deepEqual(lerpNumbers([1, 2, 3], [7], 1), [7], "dropped values go");
});

test("a stat counts up inside its own text", () => {
  assert.equal(countText("1,284 users", 0.5), "642 users");
  assert.equal(countText("$0.42", 0.5), "$0.21");
  assert.equal(countText("24%", 0), "0%");
  assert.equal(countText("Green", 0.3), "Green");
  assert.equal(countText("1,284", 1), "1,284");
  const s = spec({ type: "stat", data: { value: "1,284", label: "Users" } });
  const entering = withNumbers(s, numbersOf(s), 0.25);
  assert.equal(entering.type === "stat" && entering.stats[0].value, "321");
});
