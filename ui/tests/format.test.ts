import { test } from "node:test";
import assert from "node:assert/strict";
import { duration, plural, tokenCount } from "../src/lib/format.ts";

test("durations read the same everywhere", () => {
  assert.equal(duration(320), "320ms");
  assert.equal(duration(1400), "1.4s");
  assert.equal(duration(125_000), "2m 5s");
});

test("token counts shorten without losing scale", () => {
  assert.equal(tokenCount(950), "950");
  assert.equal(tokenCount(48_210), "48.2k");
  assert.equal(tokenCount(152_400), "152k");
  assert.equal(tokenCount(200_000), "200k");
  assert.equal(tokenCount(1_200_000), "1.2M");
});

test("plurals", () => {
  assert.equal(plural(1, "step"), "1 step");
  assert.equal(plural(3, "step"), "3 steps");
  assert.equal(plural(2, "child", "children"), "2 children");
});
