import { test } from "node:test";
import assert from "node:assert/strict";
import { NO_VEIL, stepVeil, type Veil } from "../src/lib/veil.ts";

function run(steps: [string, boolean][], from: Veil = NO_VEIL) {
  let veil = from;
  return steps.map(([url, loading]) => {
    const step = stepVeil(veil, url, loading);
    veil = step.veil;
    return step;
  });
}

test("a typed address veils its load and the end of the load lifts it", () => {
  const [typed, loading, done] = run([
    ["https://a.example/", false],
    ["https://a.example/", true],
    ["https://b.example/", false],
  ]);
  assert.equal(typed.started, true);
  assert.equal(typed.veil.fresh, true);
  assert.equal(loading.veil.fresh, true);
  assert.equal(done.finished, true);
  assert.equal(done.started, false, "the final address arrives with the end of the load");
  assert.equal(done.veil.fresh, false);
});

test("a second load soon after the first is not veiled", () => {
  const steps = run([
    ["https://a.example/", false],
    ["https://a.example/", true],
    ["https://a.example/", false],
    ["https://a.example/", true],
  ]);
  assert.equal(steps[3].veil.fresh, false);
});

test("a link clicked inside the page keeps the page in view", () => {
  const settled: Veil = { url: "https://a.example/", loading: false, fresh: false };
  const [loading, done] = run(
    [
      ["https://a.example/", true],
      ["https://a.example/next", false],
    ],
    settled,
  );
  assert.equal(loading.veil.fresh, false);
  assert.equal(done.finished, true);
  assert.equal(done.veil.fresh, false);
});

test("a new address typed while a load runs starts a fresh veil", () => {
  const busy: Veil = { url: "https://a.example/", loading: true, fresh: false };
  const [step] = run([["https://b.example/", true]], busy);
  assert.equal(step.started, true);
  assert.equal(step.veil.fresh, true);
});

test("the same values again change nothing", () => {
  const veil: Veil = { url: "https://a.example/", loading: true, fresh: true };
  const step = stepVeil(veil, veil.url, true);
  assert.deepEqual(step, { veil, started: false, finished: false });
});

test("an empty address starts nothing", () => {
  const [step] = run([["", false]]);
  assert.equal(step.started, false);
  assert.equal(step.veil.fresh, false);
});
