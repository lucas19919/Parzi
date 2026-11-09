import { test } from "node:test";
import assert from "node:assert/strict";

const store = new Map<string, string>();
(globalThis as { localStorage?: unknown }).localStorage = {
  getItem: (k: string) => store.get(k) ?? null,
  setItem: (k: string, v: string) => void store.set(k, v),
};
const { freshNotice, notifyLevel } = await import("../src/lib/notifyPrefs.ts");

test("a repeated notice shows once in ten minutes, digits aside", () => {
  assert.equal(freshNotice("s1", "Claude is retrying (attempt 1 of 5)", 0), true);
  assert.equal(freshNotice("s1", "Claude is retrying (attempt 2 of 5)", 60_000), false);
  assert.equal(freshNotice("s2", "Claude is retrying (attempt 2 of 5)", 60_000), true, "another session is its own");
  assert.equal(freshNotice("s1", "Claude is retrying (attempt 3 of 5)", 11 * 60_000), true);
  assert.equal(freshNotice("n", "", 0, 60_000), true);
  assert.equal(freshNotice("n", "", 30_000, 60_000), false);
  assert.equal(freshNotice("n", "", 61_000, 60_000), true);
});

test("notifications default to only what needs you, and the choice is kept", () => {
  let level = "";
  notifyLevel.subscribe((v) => (level = v))();
  assert.equal(level, "needed");
  notifyLevel.set("off");
  assert.equal(store.get("parzi.notify.level"), "off");
});
