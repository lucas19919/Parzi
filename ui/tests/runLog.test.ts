import { test } from "node:test";
import assert from "node:assert/strict";
import { LOG_MAX, stepLog, type RunLog } from "../src/lib/runLog.ts";

const call = (id: string, label = "") => ({ kind: "tool_call" as const, session: "s", id, name: "fs.read", label });
const result = (id: string, ok = true, ms = 10) => ({ kind: "tool_result" as const, session: "s", id, name: "fs.read", ok, ms });

test("steps are logged in order and finish with their result", () => {
  let log: RunLog | undefined;
  log = stepLog(log, call("a", "Reading a"), 1);
  log = stepLog(log, call("b"), 2);
  log = stepLog(log, result("a", true, 40), 3);
  assert.deepEqual(log?.steps.map((s) => [s.id, s.label, s.running, s.ms]), [
    ["a", "Reading a", false, 40],
    ["b", "fs.read", true, 0],
  ]);
});

test("the log stays after the run, and the next run starts a new one", () => {
  let log = stepLog(undefined, call("a"), 1);
  log = stepLog(log, { kind: "done", session: "s", turns: 1 });
  assert.equal(log?.ended, true);
  assert.equal(log?.steps.length, 1);
  assert.deepEqual([log?.steps[0].running, log?.steps[0].ok], [false, true]);
  // A step cut off by a failed or stopped run did not succeed.
  const cut = stepLog(stepLog(undefined, call("x"), 1), { kind: "error", session: "s", error: "cancelled" });
  assert.deepEqual([cut?.steps[0].running, cut?.steps[0].ok], [false, false]);
  log = stepLog(log, call("b"), 5);
  assert.deepEqual(log?.steps.map((s) => s.id), ["b"]);
  assert.equal(log?.ended, false);
});

test("other events and unknown results leave the log as it was", () => {
  const log = stepLog(undefined, call("a"), 1);
  assert.equal(stepLog(log, { kind: "text", session: "s", text: "hi" }), log);
  assert.equal(stepLog(log, result("zzz")), log);
  assert.equal(stepLog(log, call("a")), log);
  assert.equal(stepLog(undefined, result("a")), undefined);
});

test("a long run keeps its latest steps", () => {
  let log: RunLog | undefined;
  for (let i = 0; i < LOG_MAX + 5; i++) log = stepLog(log, call(`t${i}`), i);
  assert.equal(log?.steps.length, LOG_MAX);
  assert.equal(log?.steps[0].id, "t5");
});
