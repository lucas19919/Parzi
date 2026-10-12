import { test } from "node:test";
import assert from "node:assert/strict";
import { ALL, board, columnOf, dropped, homeTasks, inScope, moved, needCandidates, nextStatus, projectFor, scopeOptions, upsert, waitsOn } from "../src/lib/taskModel.ts";
import type { Task } from "../src/lib/api.ts";

function task(id: string, over: Partial<Task> = {}): Task {
  return {
    id,
    title: id.toUpperCase(),
    notes: "",
    status: "todo",
    project: "parzi",
    needs: [],
    lane: "",
    session: null,
    created: 1,
    updated: 1,
    created_by: "you",
    order: 1,
    ready: true,
    blocked_by: [],
    ...over,
  };
}

test("a created task the refresh already listed is not added twice", () => {
  const listed = [task("a"), task("b")];
  assert.deepEqual(upsert(listed, task("b", { title: "New" })).map((t) => [t.id, t.title]), [["a", "A"], ["b", "New"]]);
  assert.deepEqual(upsert(listed, task("c")).map((t) => t.id), ["a", "b", "c"]);
});

test("failed tasks wait in the Review column", () => {
  assert.equal(columnOf("failed"), "review");
  assert.equal(columnOf("doing"), "doing");
});

test("the board sorts each column and puts the newest done first", () => {
  const list = [
    task("b", { order: 2 }),
    task("a", { order: 1 }),
    task("x", { status: "done", updated: 5 }),
    task("y", { status: "done", updated: 9 }),
    task("f", { status: "failed" }),
    task("inbox", { project: null }),
  ];
  const b = board(list, "parzi");
  assert.deepEqual(b.todo.map((t) => t.id), ["a", "b"]);
  assert.deepEqual(b.done.map((t) => t.id), ["y", "x"]);
  assert.deepEqual(b.review.map((t) => t.id), ["f"]);
  assert.equal(board(list, "").todo.length, 1, "the Inbox is its own scope");
  assert.equal(board(list, ALL).todo.length, 3);
});

test("scopes match slugs without case, and All creates in the Inbox", () => {
  assert.ok(inScope(task("a", { project: "Parzi" }), "parzi"));
  assert.ok(!inScope(task("a", { project: null }), "parzi"));
  assert.ok(inScope(task("a", { project: null }), ""));
  assert.equal(projectFor(ALL), null);
  assert.equal(projectFor(""), null);
  assert.equal(projectFor("parzi"), "parzi");
});

test("blocked tasks say what they wait on", () => {
  const all = [task("a", { title: "Write the parser" }), task("b", { blocked_by: ["a", "c"] })];
  assert.equal(waitsOn(all[1], all), "Waits on Write the parser and 1 more");
  assert.equal(waitsOn(all[0], all), "");
});

test("Home shows what came back first, then what is ready", () => {
  const list = [
    task("ready2", { order: 2 }),
    task("ready1", { order: 1 }),
    task("blocked", { ready: false, blocked_by: ["x"] }),
    task("old", { status: "review", updated: 1 }),
    task("new", { status: "failed", updated: 5 }),
    task("done", { status: "done", ready: false }),
  ];
  assert.deepEqual(homeTasks(list).map((t) => t.id), ["new", "old", "ready1", "ready2"]);
  assert.equal(homeTasks(list, 2).length, 2);
});

test("status steps follow the columns", () => {
  assert.equal(nextStatus("todo", 1), "doing");
  assert.equal(nextStatus("todo", -1), null);
  assert.equal(nextStatus("done", 1), null);
  assert.equal(nextStatus("failed", -1), "doing");
});

test("moving and dropping reorder one column", () => {
  const col = [task("a"), task("b"), task("c")];
  assert.deepEqual(moved(col, "b", -1), ["b", "a", "c"]);
  assert.equal(moved(col, "a", -1), null);
  assert.equal(moved(col, "c", 1), null);
  assert.deepEqual(dropped(col, "c", "a"), ["c", "a", "b"]);
  assert.deepEqual(dropped(col, "a", null), ["b", "c", "a"]);
});

test("a task cannot need itself or anything that waits on it", () => {
  const all = [
    task("a"),
    task("b", { needs: ["a"] }),
    task("c", { needs: ["b"] }),
    task("d"),
    task("other", { project: "elsewhere" }),
  ];
  assert.deepEqual(needCandidates(all[0], all).map((t) => t.id), ["d"]);
  assert.deepEqual(needCandidates(all[2], all).map((t) => t.id).sort(), ["a", "b", "d"]);
});

test("the filter lists All, the Inbox, then projects by name", () => {
  const list = [task("a"), task("b", { project: null }), task("c", { project: "zeta" }), task("d", { status: "done" })];
  const opts = scopeOptions(list, [{ slug: "parzi", title: "Parzi" }, { slug: "alpha", title: "Alpha" }]);
  assert.deepEqual(
    opts.map((o) => [o.value, o.label, o.count]),
    [
      [ALL, "All projects", 3],
      ["", "Inbox", 1],
      ["alpha", "Alpha", 0],
      ["parzi", "Parzi", 1],
      ["zeta", "zeta", 1],
    ],
  );
});
