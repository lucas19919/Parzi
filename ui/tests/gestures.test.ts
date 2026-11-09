import { test } from "node:test";
import assert from "node:assert/strict";
import {
  WINDOW_DRAG_SKIP,
  buttonHeld,
  pressDrag,
  tabDragStarted,
  tabRelease,
  windowDown,
  windowMove,
} from "../src/lib/gestures.ts";

const hit = (piece: string) => ({
  closest(sel: string) {
    return sel === WINDOW_DRAG_SKIP && sel.includes(piece) ? {} : null;
  },
});

const plain = { closest: () => null };

test("the window drags from empty chrome only", () => {
  assert.equal(windowDown(0, 1, plain), "arm");
  assert.equal(windowDown(0, 1, null), "ignore");
  assert.equal(windowDown(0, 1, {}), "ignore");
  assert.equal(windowDown(2, 1, plain), "ignore");
  assert.equal(windowDown(0, 2, plain), "maximize");
  for (const piece of ["button", "[role='dialog']", "[role='menu']", ".dropdown", ".tab", ".tabs", ".picker", "textarea", "select"]) {
    assert.equal(windowDown(0, 1, hit(piece)), "ignore", piece);
  }
});

test("a released button never starts a window drag", () => {
  assert.equal(windowMove(0, 40), "drop");
  assert.equal(windowMove(2, 40), "drop");
  assert.equal(windowMove(1, 3), "wait");
  assert.equal(windowMove(1, 4), "drag");
  assert.equal(windowMove(3, 4), "drag");
});

test("only the main button holds a drag", () => {
  assert.equal(buttonHeld(0), false);
  assert.equal(buttonHeld(2), false);
  assert.equal(buttonHeld(1), true);
});

test("tabs ignore jitter, then select when a drag does not reorder", () => {
  assert.equal(tabDragStarted(0, 0, 4, 30), false);
  assert.equal(tabDragStarted(0, 0, 5, 0), true);

  const ids = ["a", "b", "c"];
  assert.deepEqual(tabRelease({ pressId: "b", dragId: "", order: null, ids }), { kind: "pending" });
  assert.deepEqual(tabRelease({ pressId: "b", dragId: "b", order: ["a", "b", "c"], ids }), { kind: "select", id: "b" });
  assert.deepEqual(tabRelease({ pressId: "b", dragId: "b", order: ["b", "a", "c"], ids }), { kind: "move", id: "b", to: 0 });
  assert.deepEqual(tabRelease({ pressId: "a", dragId: "a", order: ["b", "c", "a"], ids }), { kind: "move", id: "a", to: 2 });
  assert.deepEqual(tabRelease({ pressId: "", dragId: "", order: null, ids }), { kind: "none" });
});

function pointer(type: string, over: Record<string, number> = {}) {
  return Object.assign(new Event(type), { button: 0, buttons: 1, pointerId: 1, clientX: 0, clientY: 0, ...over });
}

function rig() {
  const win = new EventTarget();
  (globalThis as { window?: unknown }).window = win;
  const node = new EventTarget();
  const log: string[] = [];
  const action = pressDrag<{ x: number }>(node as unknown as HTMLElement, {
    press: (e) => (e.clientX < 0 ? null : { x: e.clientX }),
    starts: (p, e) => Math.abs(e.clientX - p.x) >= 5,
    start: () => log.push("start"),
    drag: (_, e) => log.push(`drag ${e.clientX}`),
    release: (_, e, dragged) => log.push(`release ${dragged ? "drag" : "click"}`),
    cancel: () => log.push("cancel"),
  });
  const send = (target: EventTarget, type: string, over?: Record<string, number>) => target.dispatchEvent(pointer(type, over));
  return { win, node, log, action, send };
}

test("a press-drag stays a click under the slop and ends on release", () => {
  const { win, node, log, send } = rig();
  send(node, "pointerdown");
  send(win, "pointermove", { clientX: 4 });
  send(win, "pointerup", { clientX: 4, buttons: 0 });
  assert.deepEqual(log, ["release click"]);
  send(win, "pointermove", { clientX: 40 });
  send(win, "pointerup", { buttons: 0 });
  assert.deepEqual(log, ["release click"]);
});

test("a press-drag drags past the slop, once started", () => {
  const { win, node, log, send } = rig();
  send(node, "pointerdown");
  send(win, "pointermove", { clientX: 6 });
  send(win, "pointermove", { clientX: 2 });
  send(win, "pointerup", { clientX: 2, buttons: 0 });
  assert.deepEqual(log, ["start", "drag 6", "drag 2", "release drag"]);
});

test("a press-drag never stays armed", () => {
  const { win, node, log, send, action } = rig();
  send(node, "pointerdown", { button: 2 });
  send(node, "pointerdown", { clientX: -1 });
  send(win, "pointerup", { buttons: 0 });
  assert.deepEqual(log, []);
  send(node, "pointerdown");
  send(win, "pointermove", { pointerId: 2, clientX: 40 });
  send(win, "pointerup", { pointerId: 2, buttons: 0 });
  assert.deepEqual(log, []);
  send(win, "pointermove", { clientX: 40, buttons: 0 });
  assert.deepEqual(log, ["release click"]);
  send(node, "pointerdown");
  send(win, "pointerdown");
  send(node, "pointerdown");
  send(win, "pointerup", { buttons: 0 });
  assert.deepEqual(log.slice(1), ["cancel", "release click"]);
  log.length = 0;
  send(node, "pointerdown");
  send(win, "lostpointercapture", { buttons: 0 });
  send(win, "lostpointercapture");
  send(node, "pointerdown");
  send(win, "blur");
  send(node, "pointerdown");
  action.destroy();
  send(node, "pointerdown");
  send(win, "pointerup", { buttons: 0 });
  assert.deepEqual(log, ["cancel", "cancel", "cancel"]);
});
