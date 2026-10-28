import { test } from "node:test";
import assert from "node:assert/strict";
import { dropId, enqueue, heldLabel, moveHeld, moveId, putFront, takeAll, takeHead, takeId, type HeldItem, type HoldQueue } from "../src/lib/held.ts";

const item = (text: string, extra: Partial<HeldItem> = {}): Omit<HeldItem, "id"> & { id?: string } => ({
  tab: "tab",
  text,
  quotes: [],
  files: [],
  model: "grok/grok-4.7",
  effort: "medium",
  mode: "supervised",
  cwd: "D:/Repos/Parzi",
  remote: false,
  ...extra,
});

test("each follow-up stays its own turn, in the order it was typed", () => {
  let queue: HoldQueue = {};
  queue = enqueue(queue, "s", item("first", { id: "a" })).queue;
  queue = enqueue(queue, "s", item("second", { id: "b" })).queue;
  assert.deepEqual(queue.s.map((entry) => entry.text), ["first", "second"]);
  const head = takeHead(queue, "s");
  assert.equal(head.item?.text, "first");
  assert.deepEqual(head.queue.s.map((entry) => entry.text), ["second"]);
});

test("a full queue keeps the draft instead of dropping or merging it", () => {
  let queue: HoldQueue = {};
  for (let i = 0; i < 8; i++) queue = enqueue(queue, "s", item(`m${i}`, { id: String(i) }), 8).queue;
  const refused = enqueue(queue, "s", item("one more", { id: "x" }), 8);
  assert.equal(refused.ok, false);
  assert.equal(refused.queue.s.length, 8);
  assert.equal(refused.queue.s.some((entry) => entry.text === "one more"), false);
});

test("send now, edit and side task take one message and leave the rest", () => {
  let queue: HoldQueue = {};
  queue = enqueue(queue, "s", item("first", { id: "a" })).queue;
  queue = enqueue(queue, "s", item("second", { id: "b" })).queue;
  queue = enqueue(queue, "s", item("third", { id: "c" })).queue;
  const mid = takeId(queue, "s", "b");
  assert.equal(mid.item?.id, "b");
  assert.deepEqual(mid.queue.s.map((entry) => entry.id), ["a", "c"]);
  const gone = takeId(mid.queue, "s", "missing");
  assert.equal(gone.item, undefined);
  assert.equal(gone.queue, mid.queue);
});

test("a failed send returns to the front without duplicating", () => {
  let queue: HoldQueue = {};
  queue = enqueue(queue, "s", item("waiting", { id: "b" })).queue;
  const failed: HeldItem = { ...item("failed"), id: "a" };
  queue = putFront(queue, "s", failed);
  queue = putFront(queue, "s", failed);
  assert.deepEqual(queue.s.map((entry) => entry.id), ["a", "b"]);
});

test("stopping hands every queued message back, first first", () => {
  let queue: HoldQueue = {};
  queue = enqueue(queue, "s", item("first", { id: "a" })).queue;
  queue = enqueue(queue, "s", item("second", { id: "b" })).queue;
  const all = takeAll(queue, "s");
  assert.deepEqual(all.items.map((entry) => entry.id), ["a", "b"]);
  assert.equal(all.queue.s, undefined);
  assert.deepEqual(takeAll(all.queue, "s").items, []);
});

test("a session that continues under a new id keeps its queue", () => {
  let queue: HoldQueue = {};
  queue = enqueue(queue, "old", item("follow up", { id: "a" })).queue;
  queue = enqueue(queue, "new", item("already there", { id: "b" })).queue;
  queue = moveHeld(queue, "old", "new");
  assert.equal(queue.old, undefined);
  assert.deepEqual(queue.new.map((entry) => entry.id), ["b", "a"]);
  assert.equal(moveHeld(queue, "new", "new"), queue);
});

test("a message that is only quoted plan items still has a label", () => {
  assert.equal(heldLabel({ text: "  ", quotes: [{ title: "Ship the queue" }, { title: "Add a test" }] }), "Ship the queue, Add a test");
  assert.equal(heldLabel({ text: "Fix the omnibar", quotes: [{ title: "ignored" }] }), "Fix the omnibar");
});

test("dropping one queued message leaves the rest in order", () => {
  let queue: HoldQueue = {};
  queue = enqueue(queue, "s", item("first", { id: "a" })).queue;
  queue = enqueue(queue, "s", item("second", { id: "b" })).queue;
  queue = enqueue(queue, "s", item("third", { id: "c" })).queue;
  queue = dropId(queue, "s", "b");
  assert.deepEqual(queue.s.map((entry) => entry.id), ["a", "c"]);
  assert.equal(dropId(queue, "s", "missing"), queue);
});

test("reordering clamps to the ends and leaves others in order", () => {
  let queue: HoldQueue = {};
  queue = enqueue(queue, "s", item("first", { id: "a" })).queue;
  queue = enqueue(queue, "s", item("second", { id: "b" })).queue;
  queue = enqueue(queue, "s", item("third", { id: "c" })).queue;
  queue = moveId(queue, "s", "c", 0);
  assert.deepEqual(queue.s.map((entry) => entry.id), ["c", "a", "b"]);
  queue = moveId(queue, "s", "a", 99);
  assert.deepEqual(queue.s.map((entry) => entry.id), ["c", "b", "a"]);
  assert.equal(moveId(queue, "s", "missing", 0), queue);
});
