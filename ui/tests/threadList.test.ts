import test from "node:test";
import assert from "node:assert/strict";
import { coalesce } from "../src/lib/threadList.ts";

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
    raw += 1;
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
