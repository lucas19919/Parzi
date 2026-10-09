import { test } from "node:test";
import assert from "node:assert/strict";
import { laneIcon, normLane } from "../src/lib/lanes.ts";

test("normLane maps legacy names", () => {
  assert.equal(normLane("code"), "build");
  assert.equal(normLane("research"), "work");
  assert.equal(normLane("build"), "build");
  assert.equal(normLane("work"), "work");
  assert.equal(normLane(""), "");
});

test("laneIcon maps to brain/bot/chat", () => {
  assert.equal(laneIcon("work"), "brain");
  assert.equal(laneIcon("research"), "brain");
  assert.equal(laneIcon("build"), "bot");
  assert.equal(laneIcon("code"), "bot");
  assert.equal(laneIcon(""), "chat");
});
