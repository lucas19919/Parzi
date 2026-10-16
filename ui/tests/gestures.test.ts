import { test } from "node:test";
import assert from "node:assert/strict";
import {
  WINDOW_DRAG_SKIP,
  buttonHeld,
  dragPast,
  noteMoveStarts,
  noteRelease,
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
  for (const piece of ["button", "[role='dialog']", "[role='menu']", ".noti-popover", ".dropdown", ".tab", ".picker", "textarea", "select"]) {
    assert.equal(windowDown(0, 1, hit(piece)), "ignore", piece);
  }
});

test("a released button never starts a window drag", () => {
  // The freeze: startDragging after mouseup leaves Windows in a drag loop
  // until some later click. Drop the gesture instead.
  assert.equal(windowMove(0, 40), "drop");
  assert.equal(windowMove(2, 40), "drop");
  assert.equal(windowMove(1, 3), "wait");
  assert.equal(windowMove(1, 4), "drag");
  assert.equal(windowMove(3, 4), "drag");
});

test("pointer capture waits until the drag is real", () => {
  assert.equal(dragPast(0), false);
  assert.equal(dragPast(3), false);
  assert.equal(dragPast(4), true);
  assert.equal(buttonHeld(0), false);
  assert.equal(buttonHeld(2), false);
  assert.equal(buttonHeld(1), true);
});

test("tabs ignore jitter, then select when a drag does not reorder", () => {
  assert.equal(tabDragStarted(0, 0, 4, 30, false), false);
  assert.equal(tabDragStarted(0, 0, 5, 0, false), true);
  assert.equal(tabDragStarted(10, 10, 10, 14, true), false);
  assert.equal(tabDragStarted(10, 10, 10, 15, true), true);
  // Horizontal jitter must not start a vertical tab drag.
  assert.equal(tabDragStarted(0, 0, 20, 0, true), false);

  const ids = ["a", "b", "c"];
  // No drag: the click handler selects. Releasing must not select again.
  assert.deepEqual(tabRelease({ pressId: "b", dragId: "", order: null, ids }), { kind: "pending" });
  // Dragged but put back: capture swallowed the click, so the release selects.
  assert.deepEqual(tabRelease({ pressId: "b", dragId: "b", order: ["a", "b", "c"], ids }), { kind: "select", id: "b" });
  assert.deepEqual(tabRelease({ pressId: "b", dragId: "b", order: ["b", "a", "c"], ids }), { kind: "move", id: "b", to: 0 });
  assert.deepEqual(tabRelease({ pressId: "a", dragId: "a", order: ["b", "c", "a"], ids }), { kind: "move", id: "a", to: 2 });
  assert.deepEqual(tabRelease({ pressId: "", dragId: "", order: null, ids }), { kind: "none" });
});

test("a shaky click on a note still opens it", () => {
  const path = "notes/a.md";
  // Still on the row: the folder underneath must not turn the click into a drag.
  assert.equal(noteMoveStarts(6, path, path), false);
  assert.equal(noteMoveStarts(4, null, path), false);
  assert.equal(noteMoveStarts(5, "notes/b.md", path), true);
  assert.equal(noteMoveStarts(8, null, path), true);

  const press = { path, from: "parzi" };
  assert.deepEqual(noteRelease(press, false, "", path, false), { kind: "open", path });
  // Moved, released on the same row, drop target is the folder it already lives in.
  assert.deepEqual(noteRelease(press, true, "parzi", path, false), { kind: "open", path });
  assert.deepEqual(noteRelease(press, true, "other", "notes/b.md", false), {
    kind: "move",
    path,
    from: "parzi",
    to: "other",
    copy: false,
  });
  assert.deepEqual(noteRelease(press, true, "other", "notes/b.md", true), {
    kind: "move",
    path,
    from: "parzi",
    to: "other",
    copy: true,
  });
  // Dragged onto a sibling and let go: not a click on the original.
  assert.deepEqual(noteRelease(press, true, "parzi", "notes/b.md", false), { kind: "cancel" });
  assert.deepEqual(noteRelease(press, true, "", null, false), { kind: "cancel" });
  // Released off the row without ever dragging.
  assert.deepEqual(noteRelease(press, false, "", null, false), { kind: "cancel" });
});
