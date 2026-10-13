import { test } from "node:test";
import assert from "node:assert/strict";
import { arrange, dropGone, isWidgetNote, moveBy, moveTo, vizBlock, widgetTitle, withShown } from "../src/lib/homeLayout.ts";

const BUILT_IN = ["recent", "agents", "projects", "git", "server", "sites"];

test("the saved order holds; new cards join at the end, gone ones drop out", () => {
  // Nothing saved: built-ins, then custom widgets.
  assert.deepEqual(arrange([], [...BUILT_IN, "widgets/prs.md"]), [...BUILT_IN, "widgets/prs.md"]);
  // A custom widget moved first keeps its place; a new one is appended.
  const saved = ["widgets/prs.md", "git", "recent", "agents", "projects", "server", "sites"];
  assert.deepEqual(arrange(saved, [...BUILT_IN, "widgets/prs.md", "widgets/release.md"]), [...saved, "widgets/release.md"]);
  // A deleted widget (and a card that no longer exists) leave the order.
  assert.deepEqual(arrange(["widgets/old.md", "tasks", "sites", "recent"], ["recent", "sites", "git"]), ["sites", "recent", "git"]);
  // Duplicates in a hand-edited preference count once.
  assert.deepEqual(arrange(["git", "git", "recent"], ["recent", "git"]), ["git", "recent"]);
});

test("cards move to a slot or one step, clamped at the ends", () => {
  const o = ["a", "b", "c", "d"];
  assert.deepEqual(moveTo(o, "a", 2), ["b", "c", "a", "d"]);
  assert.deepEqual(moveTo(o, "d", 0), ["d", "a", "b", "c"]);
  assert.deepEqual(moveTo(o, "b", 99), ["a", "c", "d", "b"]);
  assert.equal(moveTo(o, "b", 1), o);
  assert.equal(moveTo(o, "x", 0), o);
  assert.deepEqual(moveBy(o, "b", -1), ["b", "a", "c", "d"]);
  assert.deepEqual(moveBy(o, "c", 1), ["a", "b", "d", "c"]);
  assert.deepEqual(moveBy(o, "a", -1), o);
});

test("dragging among shown cards leaves hidden ones in their places", () => {
  // "b" is hidden; the shown cards are a, c, d and c was dragged first.
  assert.deepEqual(withShown(["a", "b", "c", "d"], ["c", "a", "d"]), ["c", "b", "a", "d"]);
  assert.deepEqual(withShown(["a", "b"], ["a", "b"]), ["a", "b"]);
});

test("custom widgets that are gone leave the order and the hidden list; built-ins stay", () => {
  const list = ["git", "widgets/prs.md", "widgets/old.md", "server"];
  assert.deepEqual(dropGone(list, ["widgets/prs.md"]), ["git", "widgets/prs.md", "server"]);
  // Unchanged lists come back as they were (no store write).
  const same = ["git", "widgets/prs.md"];
  assert.equal(dropGone(same, ["widgets/prs.md"]), same);
});

test("widget notes live under widgets/ and hold a viz block", () => {
  assert.ok(isWidgetNote("widgets/open-prs.md"));
  assert.ok(isWidgetNote("widgets/team/load.md"));
  assert.ok(!isWidgetNote("notes/widgets/open-prs.md"));
  assert.ok(!isWidgetNote("widgets/open-prs.txt"));
  assert.ok(!isWidgetNote("widgets/open-prs.conflict-laptop.md"));

  const note = '---\ntitle: Open PRs\n---\nBy repo.\n\n```viz\n{"type": "bars", "data": {"parzi": 4}}\n```\n\n```viz\n{"type": "stat"}\n```\n';
  assert.equal(vizBlock(note), '{"type": "bars", "data": {"parzi": 4}}');
  assert.equal(vizBlock("```viz\r\n[1]\r\n```"), "[1]");
  assert.equal(vizBlock("~~~viz\n{}\n~~~"), "{}");
  assert.equal(vizBlock("```json\n{}\n```"), null);
  assert.equal(vizBlock("No chart here."), null);
});

test("a widget is named by its note, else its chart, else its file", () => {
  assert.equal(widgetTitle("Open pull requests", "widgets/open-prs.md", "PRs"), "Open pull requests");
  // A note without a title of its own is titled by its file name.
  assert.equal(widgetTitle("release", "widgets/release.md", "Release 0.1.28"), "Release 0.1.28");
  assert.equal(widgetTitle("open-prs", "widgets/open-prs.md"), "Open prs");
  assert.equal(widgetTitle("", "widgets/team_load.md"), "Team load");
});
