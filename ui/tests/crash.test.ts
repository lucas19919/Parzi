import { test } from "node:test";
import assert from "node:assert/strict";
import { allowReport, describeError, isNoise } from "../src/lib/crash.ts";

test("an exception reports as an error with its stack; a backend's message as a warning", () => {
  const e = new TypeError("Cannot read properties of undefined (reading 'split')");
  const log = describeError(e);
  assert.equal(log.level, "error");
  assert.equal(log.message, "TypeError: Cannot read properties of undefined (reading 'split')");
  assert.ok(log.stack.includes("TypeError"));
  assert.deepEqual(describeError("session not found"), { level: "warn", message: "session not found", stack: "" });
  assert.deepEqual(describeError({ code: 7 }), { level: "warn", message: '{"code":7}', stack: "" });
  assert.equal(describeError(undefined).level, "warn");
});

test("reports are rate limited, and the browser's resize notices aren't faults", () => {
  const sent: number[] = [];
  const at = 1_000_000;
  for (let i = 0; i < 5; i++) assert.ok(allowReport(sent, at + i));
  assert.ok(!allowReport(sent, at + 10));
  assert.ok(allowReport(sent, at + 60_000));
  assert.ok(isNoise("ResizeObserver loop completed with undelivered notifications."));
  assert.ok(!isNoise("TypeError: x is undefined"));
});
