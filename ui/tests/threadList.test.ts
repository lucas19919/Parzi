// Run: node --test ui/tests/threadList.test.ts   (node >= 23 strips the types)
// Lives outside ui/src so svelte-check (include: src/**/*) stays node-free.
import test from "node:test";
import assert from "node:assert/strict";
import { coalesce, changesThreadList } from "../src/lib/threadList.ts";

/** Let the coalescer's promise chain settle (setImmediate is not mocked). */
const flush = () => new Promise((r) => setImmediate(r));

test("a synchronous burst of 300 events costs one call", async () => {
  let calls = 0;
  const refresh = coalesce(async () => {
    calls += 1;
  }, 100);
  for (let i = 0; i < 300; i++) void refresh();
  await refresh();
  assert.equal(calls, 1);
});

test("300 events over 3 s: 300 calls raw, ~30 coalesced", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout", "Date"] });
  let raw = 0;
  let calls = 0;
  const refresh = coalesce(async () => {
    calls += 1;
  }, 100);
  for (let i = 0; i < 300; i++) {
    raw += 1; // what the un-debounced handler did: one list_threads per event
    void refresh();
    t.mock.timers.tick(10);
    await flush();
  }
  t.mock.timers.tick(100);
  await flush();
  assert.equal(raw, 300);
  assert.ok(calls >= 28 && calls <= 32, `expected ~30 coalesced calls, got ${calls}`);
});

test("awaiting resolves only after the run that covers the call", async () => {
  let value = 0;
  const refresh = coalesce(async () => {
    await new Promise((r) => setTimeout(r, 5));
    value += 1;
  }, 10);
  await refresh();
  assert.equal(value, 1);
  // A call made while the first run is still in flight gets its own later run.
  const p = refresh();
  void refresh();
  await p;
  assert.equal(value, 2);
});

test("calls arriving mid-flight collapse into a single follow-up", async () => {
  let calls = 0;
  let release: (() => void) | null = null;
  const refresh = coalesce(async () => {
    calls += 1;
    await new Promise<void>((r) => (release = r));
  }, 10);
  const first = refresh();
  await new Promise((r) => setTimeout(r, 20));
  assert.equal(calls, 1);
  for (let i = 0; i < 50; i++) void refresh();
  release?.();
  await first;
  await new Promise((r) => setTimeout(r, 40));
  assert.equal(calls, 2);
  release?.();
});

// The whole E7 path: 300 run events for a background session, driven through
// the same predicate + coalescer App.svelte uses. Before E7 the handler called
// loadThreads() once per event; this asserts what it costs now.
test("a 300-event background run costs 300 list_threads before, 2 after", async (t) => {
  t.mock.timers.enable({ apis: ["setTimeout", "Date"] });
  const burst = [...Array.from({ length: 297 }, () => "text"), "tool_call", "usage", "done"];
  let before = 0;
  let after = 0;
  const refresh = coalesce(async () => {
    after += 1;
  }, 100);
  const listed = new Set<string>();
  for (const kind of burst) {
    before += 1; // old handler: one IPC per event for a non-active session
    const first = !listed.has("s1");
    if (kind === "done" || kind === "error") listed.delete("s1");
    else listed.add("s1");
    if (changesThreadList(kind, first)) void refresh();
    t.mock.timers.tick(10);
    await flush();
  }
  t.mock.timers.tick(200);
  await flush();
  assert.equal(before, 300);
  assert.equal(after, 2); // one when the run starts, one when it ends
});
