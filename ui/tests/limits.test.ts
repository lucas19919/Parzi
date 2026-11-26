import { test } from "node:test";
import assert from "node:assert/strict";
import { limitsFor, ringWindow, windowName } from "../src/lib/limits.ts";

const claude = [
  { label: "Session", used_percent: 22 },
  { label: "Weekly", used_percent: 61 },
  { label: "Weekly Opus", used_percent: 90, models: ["opus"] },
  { label: "Weekly Sonnet", used_percent: 10, models: ["sonnet"] },
];

test("only the limits of the model in use show", () => {
  assert.deepEqual(limitsFor(claude, "claude/opus").map((w) => w.label), ["Session", "Weekly", "Weekly Opus"]);
  assert.deepEqual(limitsFor(claude, "claude/claude-sonnet-5-5").map((w) => w.label), ["Session", "Weekly", "Weekly Sonnet"]);
  assert.deepEqual(limitsFor(claude, "claude").map((w) => w.label), ["Session", "Weekly"], "no model picked: the plan-wide ones");
});

test("Antigravity's Claude/GPT quota is not a Gemini model's", () => {
  const ag = [
    { label: "Session", used_percent: 0, models: ["claude", "gpt"] },
    { label: "Weekly", used_percent: 0, models: ["claude", "gpt"] },
  ];
  assert.deepEqual(limitsFor(ag, "antigravity/gemini-4-neon"), []);
  assert.equal(limitsFor(ag, "antigravity/gpt-oss-120b").length, 2);
});

test("the ring shows the session window, else the shortest there is", () => {
  assert.equal(ringWindow(claude)?.label, "Session");
  assert.equal(ringWindow([{ label: "Weekly", used_percent: 18 }])?.label, "Weekly");
  assert.equal(ringWindow([]), null);
});

test("Settings names a scoped window by its models", () => {
  assert.equal(windowName({ label: "Session", used_percent: 0, models: ["claude", "gpt"] }), "Claude/GPT Session");
  assert.equal(windowName({ label: "Weekly", used_percent: 0 }), "Weekly");
  assert.equal(windowName({ label: "Weekly Opus", used_percent: 0, models: ["opus"] }), "Weekly Opus", "already named");
});
