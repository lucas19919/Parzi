import { test } from "node:test";
import assert from "node:assert/strict";
import {
  echoOf, GRACE_MS, initialSends, isBusy, isPaused, isWorking, phaseOf, RETRY_MS, step, tabKey, workingSet,
  type Effect, type Hold, type SendEvent, type Sends,
} from "../src/lib/sendState.ts";

const item = (text: string, id = text): Hold => ({
  id,
  tab: "tab",
  text,
  files: [],
  model: "claude/claude-opus-5-5",
  effort: "medium",
  mode: "supervised",
  cwd: "D:/Repos/Parzi",
  remote: false,
});

/** Runs events in order and keeps every effect, like App.svelte does. */
function run(s: Sends, events: SendEvent[], now = 10_000): { s: Sends; fx: Effect[]; ok: boolean[] } {
  const fx: Effect[] = [];
  const ok: boolean[] = [];
  for (const ev of events) {
    const r = step(s, ev, now);
    s = r.state;
    fx.push(...r.effects);
    ok.push(r.ok);
  }
  return { s, fx, ok };
}

const texts = (s: Sends, key: string) => (s.queue[key] ?? []).map((h) => h.text);
const delivered = (fx: Effect[]) => fx.filter((f) => f.do === "deliver").map((f) => (f.do === "deliver" ? f.item.text : ""));

/** A session with a confirmed run going. */
function working(key = "s"): Sends {
  return run(initialSends(), [
    { type: "begin", key, text: "first", users: 0 },
    { type: "started", key, sid: key },
  ]).s;
}

test("a sent message is busy at once, then running once the engine takes it", () => {
  let s = initialSends();
  assert.equal(isBusy(s, "s"), false);
  s = step(s, { type: "begin", key: "s", text: "hi", users: 2 }, 0).state;
  assert.equal(phaseOf(s, "s"), "sending");
  assert.equal(isBusy(s, "s"), true);
  assert.equal(isWorking(s, "s"), true);
  s = step(s, { type: "started", key: "s", sid: "s" }, 0).state;
  assert.equal(phaseOf(s, "s"), "running");
  assert.deepEqual([...workingSet(s)], ["s"]);
});

test("a second send while busy is refused, so Enter queues instead", () => {
  const s = working();
  assert.equal(isBusy(s, "s"), true);
  assert.equal(step(s, { type: "begin", key: "s", text: "again", users: 1 }, 0).ok, false);
  const held = run(s, [{ type: "hold", key: "s", item: item("second") }]).s;
  assert.deepEqual(texts(held, "s"), ["second"]);
});

test("queued messages go out one per finished run, in order", () => {
  let { s } = run(working(), [
    { type: "hold", key: "s", item: item("second") },
    { type: "hold", key: "s", item: item("third") },
  ]);
  let r = run(s, [{ type: "ended", key: "s", outcome: "done" }]);
  assert.deepEqual(delivered(r.fx), ["second"]);
  assert.equal(phaseOf(r.s, "s"), "sending");
  assert.deepEqual(texts(r.s, "s"), ["third"]);
  // Nothing more goes until that one's run ends too.
  r = run(r.s, [{ type: "started", key: "s", sid: "s" }]);
  assert.deepEqual(r.fx, []);
  r = run(r.s, [{ type: "ended", key: "s", outcome: "done" }]);
  assert.deepEqual(delivered(r.fx), ["third"]);
  s = run(r.s, [{ type: "started", key: "s", sid: "s" }, { type: "ended", key: "s", outcome: "done" }]).s;
  assert.equal(phaseOf(s, "s"), "idle");
  assert.equal(s.queue.s, undefined);
});

test("a paused queue does not drain, and resuming an idle session sends its head", () => {
  let { s } = run(working(), [
    { type: "hold", key: "s", item: item("second") },
    { type: "pause", key: "s", on: true },
  ]);
  assert.equal(isPaused(s, "s"), true);
  const r = run(s, [{ type: "ended", key: "s", outcome: "done" }]);
  assert.deepEqual(r.fx, []);
  assert.deepEqual(texts(r.s, "s"), ["second"]);
  const resumed = run(r.s, [{ type: "pause", key: "s", on: false }]);
  assert.deepEqual(delivered(resumed.fx), ["second"]);
  assert.equal(isPaused(resumed.s, "s"), false);
});

test("resuming while a run goes waits for that run", () => {
  const { s, fx } = run(working(), [
    { type: "hold", key: "s", item: item("second") },
    { type: "pause", key: "s", on: true },
    { type: "pause", key: "s", on: false },
  ]);
  assert.deepEqual(fx, []);
  assert.deepEqual(delivered(run(s, [{ type: "ended", key: "s", outcome: "done" }]).fx), ["second"]);
});

test("stopping pauses the queue and never drains it", () => {
  let r = run(working(), [
    { type: "hold", key: "s", item: item("second") },
    { type: "stop", key: "s" },
  ]);
  assert.deepEqual(r.fx, [{ do: "kill", key: "s" }]);
  assert.equal(phaseOf(r.s, "s"), "stopping");
  assert.equal(isWorking(r.s, "s"), false);
  assert.equal(isBusy(r.s, "s"), true);
  assert.equal(isPaused(r.s, "s"), true);
  // The cancelled end and then the stop's return: still nothing goes.
  r = run(r.s, [
    { type: "ended", key: "s", outcome: "stopped" },
    { type: "stopped", key: "s" },
  ]);
  assert.deepEqual(r.fx, []);
  assert.equal(phaseOf(r.s, "s"), "idle");
  assert.deepEqual(texts(r.s, "s"), ["second"]);
});

test("a message typed while the stop lands goes out once it has", () => {
  let r = run(working(), [{ type: "stop", key: "s" }]);
  assert.equal(isPaused(r.s, "s"), false);
  r = run(r.s, [{ type: "hold", key: "s", item: item("instead") }]);
  r = run(r.s, [{ type: "stopped", key: "s" }]);
  assert.deepEqual(delivered(r.fx), ["instead"]);
});

test("stopping an idle session does nothing", () => {
  const r = step(initialSends(), { type: "stop", key: "s" }, 0);
  assert.equal(r.ok, false);
  assert.deepEqual(r.effects, []);
});

test("a run that exits early pauses its queue; other failures keep it waiting", () => {
  const exited = run(working(), [
    { type: "hold", key: "s", item: item("second") },
    { type: "ended", key: "s", outcome: "exited" },
  ]);
  assert.deepEqual(exited.fx, []);
  assert.equal(isPaused(exited.s, "s"), true);
  for (const outcome of ["failed", "paused", "stopped"] as const) {
    const r = run(working(), [
      { type: "hold", key: "s", item: item("second") },
      { type: "ended", key: "s", outcome },
    ]);
    assert.deepEqual(r.fx, [], outcome);
    assert.equal(isPaused(r.s, "s"), false, outcome);
    assert.deepEqual(texts(r.s, "s"), ["second"], outcome);
    assert.equal(phaseOf(r.s, "s"), "idle", outcome);
  }
});

test("send now stops the run and sends that message when the stop lands", () => {
  let r = run(working(), [
    { type: "hold", key: "s", item: item("second") },
    { type: "hold", key: "s", item: item("third") },
    { type: "pause", key: "s", on: true },
    { type: "sendNow", key: "s", itemId: "third" },
  ]);
  assert.deepEqual(r.fx, [{ do: "kill", key: "s" }]);
  assert.equal(isPaused(r.s, "s"), false);
  assert.deepEqual(texts(r.s, "s"), ["second"]);
  r = run(r.s, [{ type: "ended", key: "s", outcome: "stopped" }]);
  assert.deepEqual(r.fx, []);
  r = run(r.s, [{ type: "stopped", key: "s" }]);
  assert.deepEqual(delivered(r.fx), ["third"]);
  assert.equal(phaseOf(r.s, "s"), "sending");
  assert.deepEqual(texts(r.s, "s"), ["second"]);
});

test("a stop's own end arriving after the stop returned doesn't end what goes next", () => {
  let r = run(working(), [
    { type: "hold", key: "s", item: item("now") },
    { type: "sendNow", key: "s" },
    { type: "stopped", key: "s" },
  ]);
  assert.deepEqual(delivered(r.fx), ["now"]);
  r = run(r.s, [
    { type: "ended", key: "s", outcome: "stopped" },
    { type: "started", key: "s", sid: "s" },
  ]);
  assert.equal(phaseOf(r.s, "s"), "running");
  // Only once: a later stop of the new run counts.
  r = run(r.s, [{ type: "ended", key: "s", outcome: "stopped" }]);
  assert.equal(phaseOf(r.s, "s"), "idle");
  // And not when the end came before the stop returned, or long after.
  const before = run(working(), [
    { type: "stop", key: "s" },
    { type: "ended", key: "s", outcome: "stopped" },
    { type: "stopped", key: "s" },
    { type: "begin", key: "s", text: "x", users: 0 },
    { type: "started", key: "s", sid: "s" },
    { type: "ended", key: "s", outcome: "stopped" },
  ]);
  assert.equal(phaseOf(before.s, "s"), "idle");
  const late = run(working(), [{ type: "stop", key: "s" }, { type: "stopped", key: "s" }]).s;
  assert.equal(phaseOf(step(late, { type: "activity", key: "s" }, 10_000).state, "s"), "running");
  assert.equal(phaseOf(step(step(late, { type: "activity", key: "s" }, 10_000).state, { type: "ended", key: "s", outcome: "stopped" }, 10_000 + GRACE_MS).state, "s"), "idle");
});

test("send now on an idle session sends at once; while sending it goes next", () => {
  const idle = run(initialSends(), [
    { type: "hold", key: "s", item: item("later") },
    { type: "sendNow", key: "s" },
  ]);
  assert.deepEqual(delivered(idle.fx), ["later"]);
  const busy = run(initialSends(), [
    { type: "begin", key: "s", text: "first", users: 0 },
    { type: "hold", key: "s", item: item("a") },
    { type: "hold", key: "s", item: item("b") },
    { type: "sendNow", key: "s", itemId: "b" },
  ]);
  assert.deepEqual(busy.fx, []);
  assert.deepEqual(texts(busy.s, "s"), ["b", "a"]);
});

test("the sent message stays on screen until the store has it", () => {
  let s = step(initialSends(), { type: "begin", key: "s", text: "hi", users: 3 }, 0).state;
  assert.deepEqual(echoOf(s, "s"), { text: "hi", users: 3 });
  s = step(s, { type: "started", key: "s", sid: "s" }, 0).state;
  // A reload before the run recorded it keeps the message showing.
  s = step(s, { type: "stored", key: "s", users: 3 }, 0).state;
  assert.equal(echoOf(s, "s")?.text, "hi");
  // The run ending doesn't take it away either: the reload after it does.
  s = step(s, { type: "ended", key: "s", outcome: "done" }, 0).state;
  assert.equal(echoOf(s, "s")?.text, "hi");
  s = step(s, { type: "stored", key: "s", users: 4 }, 0).state;
  assert.equal(echoOf(s, "s"), null);
});

test("a refused message leaves the screen; a still-working one waits in the queue and retries", () => {
  const refused = run(initialSends(), [
    { type: "begin", key: "s", text: "hi", users: 0 },
    { type: "failed", key: "s", stillWorking: false },
  ]);
  assert.equal(echoOf(refused.s, "s"), null);
  assert.equal(phaseOf(refused.s, "s"), "idle");
  assert.equal(refused.s.queue.s, undefined);

  let r = run(initialSends(), [
    { type: "hold", key: "s", item: item("queued") },
    { type: "begin", key: "s", text: "typed", users: 0 },
    { type: "failed", key: "s", item: item("typed"), stillWorking: true },
  ]);
  assert.deepEqual(texts(r.s, "s"), ["queued", "typed"]);
  assert.equal(phaseOf(r.s, "s"), "running");
  assert.deepEqual(r.fx, [{ do: "retry", key: "s", ms: RETRY_MS[0], tries: 1 }]);
  // Still active: wait longer. Then idle: the head goes.
  r = run(r.s, [{ type: "woke", key: "s", tries: 1, status: "active" }]);
  assert.deepEqual(r.fx, [{ do: "retry", key: "s", ms: RETRY_MS[1], tries: 2 }]);
  // A timer left over from the first wait changes nothing.
  assert.deepEqual(run(r.s, [{ type: "woke", key: "s", tries: 1, status: "idle" }]).fx, []);
  r = run(r.s, [{ type: "woke", key: "s", tries: 2, status: "idle" }]);
  assert.deepEqual(delivered(r.fx), ["queued"]);
});

test("a queued message the engine refuses goes back to the front", () => {
  let r = run(working(), [
    { type: "hold", key: "s", item: item("a") },
    { type: "hold", key: "s", item: item("b") },
    { type: "ended", key: "s", outcome: "done" },
  ]);
  const sent = r.fx.find((f) => f.do === "deliver");
  assert.ok(sent && sent.do === "deliver");
  r = run(r.s, [
    { type: "echo", key: "s", text: sent.item.text, users: 1 },
    { type: "failed", key: "s", item: sent.item, front: true, stillWorking: false },
  ]);
  assert.deepEqual(texts(r.s, "s"), ["a", "b"]);
  assert.equal(echoOf(r.s, "s"), null);
});

test("retries give up after the last wait and leave the queue for the next run end", () => {
  let r = run(working(), [
    { type: "hold", key: "s", item: item("a") },
    { type: "ended", key: "s", outcome: "done" },
    { type: "failed", key: "s", item: { ...item("a") }, front: true, stillWorking: true },
  ]);
  for (let i = 1; i < RETRY_MS.length; i++) r = run(r.s, [{ type: "woke", key: "s", tries: i, status: "active" }]);
  r = run(r.s, [{ type: "woke", key: "s", tries: RETRY_MS.length, status: "active" }]);
  assert.deepEqual(r.fx, []);
  assert.equal(isBusy(r.s, "s"), true);
  assert.deepEqual(delivered(run(r.s, [{ type: "ended", key: "s", outcome: "done" }]).fx), ["a"]);
});

test("store reads don't overrule a run event just after it", () => {
  let s = working();
  // The store still says idle right after the send.
  s = step(s, { type: "seen", list: [{ id: "s", status: "idle" }] }, 10_000 + GRACE_MS - 1).state;
  assert.equal(phaseOf(s, "s"), "running");
  s = step(s, { type: "ended", key: "s", outcome: "done" }, 20_000).state;
  // And still says active right after the end.
  s = step(s, { type: "seen", list: [{ id: "s", status: "active" }] }, 20_000 + 100).state;
  assert.equal(phaseOf(s, "s"), "idle");
  // Later reads count: someone else started it.
  s = step(s, { type: "seen", list: [{ id: "s", status: "active" }] }, 20_000 + GRACE_MS).state;
  assert.equal(phaseOf(s, "s"), "running");
  s = step(s, { type: "seen", list: [{ id: "s", status: "done" }] }, 30_000).state;
  assert.equal(phaseOf(s, "s"), "idle");
});

test("store reads never touch a send or a stop in progress", () => {
  let s = step(initialSends(), { type: "begin", key: "s", text: "hi", users: 0 }, 0).state;
  s = step(s, { type: "seen", list: [{ id: "s", status: "idle" }] }, 60_000).state;
  assert.equal(phaseOf(s, "s"), "sending");
  s = run(working(), [{ type: "stop", key: "s" }]).s;
  s = step(s, { type: "seen", list: [{ id: "s", status: "active" }, { id: "x", status: "idle" }] }, 60_000).state;
  assert.equal(phaseOf(s, "s"), "stopping");
  assert.equal(s.sessions.x, undefined);
});

test("run events mark a session working that this window didn't start", () => {
  const s = step(initialSends(), { type: "activity", key: "child" }, 0).state;
  assert.equal(isWorking(s, "child"), true);
  const again = step(s, { type: "activity", key: "child" }, 0);
  assert.equal(again.state, s);
});

test("a run that ends before the engine has answered the send ends once it does", () => {
  const r = run(initialSends(), [
    { type: "begin", key: "s", text: "quick", users: 0 },
    { type: "hold", key: "s", item: item("next") },
    { type: "ended", key: "s", outcome: "done" },
  ]);
  assert.equal(phaseOf(r.s, "s"), "sending");
  assert.deepEqual(r.fx, []);
  const after = run(r.s, [{ type: "started", key: "s", sid: "s" }]);
  assert.deepEqual(delivered(after.fx), ["next"]);
});

test("a new tab's send moves to the session the engine made, echo and all", () => {
  const key = tabKey("t1");
  let s = step(initialSends(), { type: "begin", key, text: "hello", users: 0 }, 0).state;
  assert.equal(workingSet(s).size, 0);
  s = step(s, { type: "started", key, sid: "new" }, 0).state;
  assert.equal(s.sessions[key], undefined);
  assert.equal(phaseOf(s, "new"), "running");
  assert.equal(echoOf(s, "new")?.text, "hello");
});

test("a session handed to another id keeps its queue and pause", () => {
  const { s } = run(working("old"), [
    { type: "hold", key: "old", item: item("q") },
    { type: "pause", key: "old", on: true },
    { type: "ended", key: "old", outcome: "done" },
    { type: "begin", key: "old", text: "go", users: 1 },
    { type: "started", key: "old", sid: "new" },
  ]);
  assert.deepEqual(texts(s, "new"), ["q"]);
  assert.equal(isPaused(s, "new"), true);
  assert.equal(s.queue.old, undefined);
});

test("sessions don't touch each other's state", () => {
  const { s, fx } = run(working("a"), [
    { type: "begin", key: "b", text: "other", users: 0 },
    { type: "hold", key: "a", item: item("for a") },
    { type: "ended", key: "b", outcome: "done" },
  ]);
  assert.deepEqual(fx, []);
  assert.equal(phaseOf(s, "a"), "running");
  assert.deepEqual(texts(s, "a"), ["for a"]);
});

test("queue edits: take, put back, drop and move; an empty queue is never paused", () => {
  let { s } = run(working(), [
    { type: "hold", key: "s", item: item("a") },
    { type: "hold", key: "s", item: item("b") },
    { type: "hold", key: "s", item: item("c") },
    { type: "move", key: "s", itemId: "c", to: 0 },
  ]);
  assert.deepEqual(texts(s, "s"), ["c", "a", "b"]);
  const taken = step(s, { type: "take", key: "s", itemId: "a" }, 0);
  assert.equal(taken.item?.text, "a");
  s = taken.state;
  s = step(s, { type: "putBack", key: "s", item: taken.item! }, 0).state;
  assert.deepEqual(texts(s, "s"), ["a", "c", "b"]);
  s = run(s, [
    { type: "pause", key: "s", on: true },
    { type: "drop", key: "s", itemId: "a" },
    { type: "drop", key: "s", itemId: "b" },
    { type: "drop", key: "s", itemId: "c" },
  ]).s;
  assert.equal(isPaused(s, "s"), false);
  assert.equal(step(s, { type: "take", key: "s" }, 0).ok, false);
});

test("a full queue refuses the message", () => {
  let s = working();
  for (let i = 0; i < 8; i++) s = step(s, { type: "hold", key: "s", item: item(`m${i}`) }, 0).state;
  assert.equal(step(s, { type: "hold", key: "s", item: item("one more") }, 0).ok, false);
});

test("deleting a session clears its queue, pause and state", () => {
  const r = run(working(), [
    { type: "hold", key: "s", item: item("a") },
    { type: "pause", key: "s", on: true },
    { type: "deleted", key: "s" },
  ]);
  assert.equal(r.s.queue.s, undefined);
  assert.equal(isPaused(r.s, "s"), false);
  assert.equal(phaseOf(r.s, "s"), "idle");
  assert.deepEqual(r.fx, []);
});
