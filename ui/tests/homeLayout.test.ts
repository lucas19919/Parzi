import { test } from "node:test";
import assert from "node:assert/strict";
import { arrange, CUT, fades, isWidgetNote, mergeOrder, moveBy, moveTo, nearestSlot, PEEK, peekHeight, rowsOf, vizBlock, widgetTitle, withShown } from "../src/lib/homeLayout.ts";

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

test("a card dropped after its widget was deleted loses no other card", () => {
  // "w" was deleted while "a" was dragged past it: the live order lacks it.
  assert.deepEqual(withShown(["a", "b", "c"], ["b", "w", "a", "c"]), ["b", "a", "c"]);
  assert.deepEqual(withShown(["a", "b", "c"], ["c", "c", "a", "b"]), ["c", "a", "b"]);
});

test("a new order keeps custom widgets not read yet next to the card they followed", () => {
  const saved = ["recent", "widgets/prs.md", "agents", "git"];
  // The widget's note isn't read yet, so Home only has the built-ins.
  assert.deepEqual(mergeOrder(saved, ["agents", "recent", "git"]), ["agents", "recent", "widgets/prs.md", "git"]);
  // First in the saved order: it stays first.
  assert.deepEqual(mergeOrder(["widgets/a.md", "git", "recent"], ["recent", "git"]), ["widgets/a.md", "recent", "git"]);
  assert.deepEqual(mergeOrder([], ["git", "recent"]), ["git", "recent"]);
});

test("a dragged card lands in the slot nearest its centre", () => {
  const slots = [0, 1, 2, 3].map((i) => ({ left: (i % 2) * 312, top: Math.floor(i / 2) * 184, width: 300, height: 172 }));
  // Over its own slot, then most of the way into the next one.
  assert.equal(nearestSlot(slots, 150, 86), 0);
  assert.equal(nearestSlot(slots, 150 + 200, 86), 1);
  assert.equal(nearestSlot(slots, 150, 86 + 120), 2);
  assert.equal(nearestSlot([], 0, 0), -1);
});

test("cards in a row share a row", () => {
  assert.deepEqual(rowsOf([{ top: 16, height: 172 }, { top: 16, height: 172 }, { top: 200, height: 120 }, { top: 200, height: 172 }]), [
    { top: 16, bottom: 188 },
    { top: 200, bottom: 372 },
  ]);
});

test("cards that overflow stop partway down a row, so the next ones peek out", () => {
  const rows = [
    { top: 16, bottom: 188 },
    { top: 200, bottom: 372 },
    { top: 384, bottom: 556 },
  ];
  // Everything fits: the list keeps all the room.
  assert.equal(peekHeight(700, rows, 620), 700);
  // The window already shows enough of the second row.
  assert.equal(peekHeight(280, rows, 620), 280);
  // Only a sliver of the third row (or none, in the gap) would show: the
  // second row is cut instead.
  assert.equal(peekHeight(384 + PEEK - 10, rows, 620), 372 - CUT);
  assert.equal(peekHeight(378, rows, 620), 372 - CUT);
  // The cards all fit, only the Customize row doesn't: nothing to peek.
  assert.equal(peekHeight(560, rows, 620), 560);
  // A first row cut short has no row above it to cut.
  assert.equal(peekHeight(40, rows, 620), 40);
});

test("the list's fades: the top one once scrolled, the bottom deeper while more is below", () => {
  assert.deepEqual(fades(0, 400, 400), { top: 0, bottom: 32 });
  assert.deepEqual(fades(0, 800, 400), { top: 0, bottom: 56 });
  assert.deepEqual(fades(120, 800, 400), { top: 16, bottom: 56 });
  assert.deepEqual(fades(400, 800, 400), { top: 16, bottom: 32 });
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
