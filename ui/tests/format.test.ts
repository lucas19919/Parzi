import { test } from "node:test";
import assert from "node:assert/strict";
import { ago } from "../src/lib/format.ts";

const NOW = 1_800_000_000_000;

test("ago reads short", () => {
  assert.equal(ago(NOW - 30_000, NOW), "just now");
  assert.equal(ago(NOW - 5 * 60_000, NOW), "5m ago");
  assert.equal(ago(NOW - 2 * 3_600_000, NOW), "2h ago");
  assert.equal(ago(NOW - 30 * 3_600_000, NOW), "yesterday");
  assert.match(ago(NOW - 9 * 86_400_000, NOW), /\d/);
  assert.equal(ago(0, NOW), "");
  assert.equal(ago(Number.NaN, NOW), "");
});
