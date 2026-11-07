import { test } from "node:test";
import assert from "node:assert/strict";
import { questionParts } from "../src/lib/questionText.ts";

test("a one line question has nothing under it", () => {
  assert.deepEqual(questionParts("Which branch?"), { head: "Which branch?", lines: [] });
});

test("a proposed plan reads as headings and marked work", () => {
  const parts = questionParts(
    'Change the plan "Release"?\nGoal: A calmer sidebar\nNext\n+ Plan history (Next)\n- Work view (dropped)\n~ Short names (was Session names)\n\n',
  );
  assert.equal(parts.head, 'Change the plan "Release"?');
  assert.deepEqual(parts.lines, [
    { kind: "heading", text: "Goal: A calmer sidebar" },
    { kind: "heading", text: "Next" },
    { kind: "add", text: "Plan history (Next)" },
    { kind: "drop", text: "Work view (dropped)" },
    { kind: "change", text: "Short names (was Session names)" },
  ]);
});

test("a dash inside a word is not a mark", () => {
  assert.deepEqual(questionParts("Pick one\n-rf is risky\n+1 from me").lines, [
    { kind: "heading", text: "-rf is risky" },
    { kind: "heading", text: "+1 from me" },
  ]);
});
