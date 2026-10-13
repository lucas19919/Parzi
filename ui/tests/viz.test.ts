import { test } from "node:test";
import assert from "node:assert/strict";
import { axisFloor, describe, fmt, niceTicks, parseViz, whenText, type VizSpec } from "../src/lib/viz/spec.ts";

function ok(input: unknown): VizSpec {
  const r = parseViz(input);
  if (!r.ok) assert.fail(`expected a spec, got: ${r.error}`);
  return r.spec;
}

function bad(input: unknown): string {
  const r = parseViz(input);
  if (r.ok) assert.fail(`expected an error, got a ${r.spec.type}`);
  return r.error;
}

test("bad specs come back as one readable line, never a throw", () => {
  assert.match(bad("{not json"), /isn't valid JSON/);
  assert.match(bad("[1, 2, 3]"), /one JSON object/);
  assert.match(bad(null), /one JSON object/);
  assert.match(bad({ data: [1, 2] }), /no type.*bars, line/);
  assert.match(bad({ type: "pie", data: [1, 2] }), /"pie" isn't a viz type/);
  assert.match(bad({ type: "bars" }), /bars need data/);
  assert.match(bad({ type: "bars", data: [{ label: "a", value: "lots" }] }), /bars need data/);
  assert.match(bad({ type: "line", data: [4] }), /at least two points/);
  assert.match(bad({ type: "spark", data: ["x"] }), /at least two numbers/);
  assert.match(bad({ type: "stat", data: {} }), /needs a value/);
  assert.match(bad({ type: "table", data: [] }), /a table needs/);
  // The source stays with the error, to show under it.
  const r = parseViz('{"type": "pie"}');
  assert.equal(r.ok ? "" : r.source, '{"type": "pie"}');
});

test("bars read every shape agents write", () => {
  const want = [
    { label: "parzi", value: 4 },
    { label: "memory-ai", value: 2 },
  ];
  const read = (data: unknown, extra: Record<string, unknown> = {}) => {
    const s = ok({ type: "bars", data, ...extra });
    assert.equal(s.type, "bars");
    return s.type === "bars" ? s.items : [];
  };
  assert.deepEqual(read(want), want);
  assert.deepEqual(read([["parzi", 4], ["memory-ai", "2"]]), want);
  assert.deepEqual(read({ parzi: 4, "memory-ai": 2 }), want);
  assert.deepEqual(read([{ name: "parzi", count: 4 }, { x: "memory-ai", y: 2 }]), want);
  assert.deepEqual(read([4, 2], { labels: ["parzi", "memory-ai"] }), want);
  // Items that aren't numbers drop out; the rest draw.
  assert.deepEqual(read([{ label: "parzi", value: 4 }, { label: "?", value: null }, { label: "memory-ai", value: "2" }]), want);
});

test("bars stand as columns for short labels, else lie as rows, unless told", () => {
  const orient = (spec: Record<string, unknown>) => {
    const s = ok({ type: "bars", ...spec });
    return s.type === "bars" ? s.orient : "";
  };
  assert.equal(orient({ data: { Mon: 1, Tue: 2 } }), "v");
  assert.equal(orient({ data: { "Measure height": 18, Markdown: 3 } }), "h");
  assert.equal(orient({ data: { Mon: 1 }, orient: "horizontal" }), "h");
  assert.equal(orient({ data: { parzi: 1 }, orient: "v" }), "v");
  assert.equal(orient({ type: "columns", data: { parzi: 1 } }), "v");
  assert.equal(orient({ type: "hbar", data: { Mon: 1 } }), "h");
});

test("lines take numbers, points, or named series, padded to one length", () => {
  const one = ok({ type: "line", title: "Frame time", data: [3, 5, 4] });
  assert.equal(one.type, "line");
  if (one.type !== "line") return;
  assert.deepEqual(one.labels, ["1", "2", "3"]);
  assert.deepEqual(one.series, [{ name: "Frame time", values: [3, 5, 4] }]);

  const pts = ok({ type: "line", data: [{ x: "Mon", y: 1 }, { x: "Tue", y: 2 }] });
  assert.deepEqual(pts.type === "line" && pts.labels, ["Mon", "Tue"]);

  const many = ok({
    type: "area",
    data: { labels: ["a", "b", "c"], series: [{ name: "Before", values: [1, 2, 3] }, { name: "After", values: [1, null] }] },
  });
  assert.equal(many.type, "line");
  if (many.type !== "line") return;
  assert.equal(many.area, true);
  assert.deepEqual(many.series[1].values, [1, null, null]);

  // Series past the palette fold away and say so.
  const crowd = ok({ type: "line", series: Array.from({ length: 6 }, (_, i) => ({ name: `s${i}`, values: [i, i + 1] })) });
  assert.equal(crowd.type === "line" && crowd.series.length, 4);
  assert.equal(crowd.more, 2);
});

test("stats format their numbers and deltas", () => {
  const s = ok({ type: "stat", title: "Release", data: [{ label: "Open PRs", value: 12900, delta: -3, vs: "vs last week" }, { label: "Spend", value: 4200000, unit: "$" }, { label: "Mood", value: "good", delta: "+2" }] });
  assert.equal(s.type, "stat");
  if (s.type !== "stat") return;
  assert.deepEqual(
    s.stats.map((x) => [x.label, x.value, x.delta, x.dir]),
    [
      ["Open PRs", "12.9k", "−3 vs last week", -1],
      ["Spend", "$4.2M", "", 0],
      ["Mood", "good", "+2", 1],
    ],
  );
  // Top-level fields are one stat; its title stays the title, not its label.
  const top = ok({ type: "kpi", title: "Tests", value: 412, delta: 18, spark: [1, 2, 3] });
  assert.deepEqual(top.type === "stat" && top.stats, [{ label: "", value: "412", delta: "+18", dir: 1, spark: [1, 2, 3] }]);
  assert.equal(top.title, "Tests");
});

test("progress reads fractions, percents and parts of a max", () => {
  const s = ok({ type: "progress", data: [{ label: "a", value: 0.42 }, { label: "b", value: 42 }, { label: "c", value: 3, max: 9 }, { label: "d", value: 1.5 }] });
  assert.equal(s.type, "progress");
  if (s.type !== "progress") return;
  assert.deepEqual(
    s.items.map((p) => [p.label, +p.value.toFixed(3), p.text]),
    [
      ["a", 0.42, "42%"],
      ["b", 0.42, "42%"],
      ["c", 0.333, "3 of 9"],
      ["d", 0.015, "2%"],
    ],
  );
  const single = ok({ type: "meter", value: 0.5 });
  assert.equal(single.type === "progress" && single.items[0].text, "50%");
});

test("tables come from columns and rows or from objects", () => {
  const a = ok({ type: "table", columns: ["Repo", "PRs"], rows: [["parzi", 4], ["thesis", 12345]] });
  assert.equal(a.type, "table");
  if (a.type !== "table") return;
  assert.deepEqual(a.columns, ["Repo", "PRs"]);
  assert.deepEqual(a.numeric, [false, true]);
  // Exact and grouped in a table, not rounded like a chart.
  assert.deepEqual(a.rows, [["parzi", "4"], ["thesis", "12,345"]]);

  const b = ok({ type: "table", data: [{ Repo: "parzi", PRs: 4 }, { Repo: "thesis", Owner: "me" }] });
  assert.equal(b.type, "table");
  if (b.type !== "table") return;
  assert.deepEqual(b.columns, ["Repo", "PRs", "Owner"]);
  assert.deepEqual(b.rows, [["parzi", "4", ""], ["thesis", "", "me"]]);

  const long = ok({ type: "table", data: Array.from({ length: 130 }, (_, i) => [i]) });
  assert.equal(long.type === "table" && long.rows.length, 100);
  assert.equal(long.more, 30);
});

test("lists and timelines read strings, objects and status words", () => {
  const l = ok({ type: "list", data: ["Plain", { title: "cargo test", detail: "workspace", value: 42, status: "passed" }, { label: "Release", status: "failed" }, { label: "Odd", tone: "purple" }] });
  assert.equal(l.type, "list");
  if (l.type !== "list") return;
  assert.deepEqual(
    l.items.map((i) => [i.label, i.detail, i.value, i.tone, i.status]),
    [
      ["Plain", "", "", "", ""],
      ["cargo test", "workspace", "42", "ok", "passed"],
      ["Release", "", "", "bad", "failed"],
      ["Odd", "", "", "", "purple"],
    ],
  );
  const t = ok({ type: "timeline", data: [{ date: "2026-10-12T09:30", title: "Merged", state: "active" }, "Started"] });
  assert.equal(t.type, "timeline");
  if (t.type !== "timeline") return;
  assert.deepEqual(
    t.items.map((i) => [i.when, i.label, i.tone]),
    [
      ["2026-10-12T09:30", "Merged", "accent"],
      ["", "Started", ""],
    ],
  );
});

test("numbers read short, with their unit where it belongs", () => {
  assert.equal(fmt(0), "0");
  assert.equal(fmt(1234), "1,234");
  assert.equal(fmt(12345), "12.3k");
  assert.equal(fmt(123456), "123k");
  assert.equal(fmt(4_200_000, "$"), "$4.2M");
  assert.equal(fmt(-4_200_000, "$"), "-$4.2M");
  assert.equal(fmt(3_000_000_000), "3B");
  assert.equal(fmt(0.456), "0.46");
  assert.equal(fmt(41.25, "s"), "41.25 s");
  assert.equal(fmt(38, "%"), "38%");
  assert.equal(fmt(38, "ms"), "38 ms");
});

test("axes tick at round numbers and start at zero unless the data is far off it", () => {
  assert.deepEqual(niceTicks(0, 38), [0, 10, 20, 30, 40]);
  assert.deepEqual(niceTicks(0, 40), [0, 10, 20, 30, 40]);
  assert.deepEqual(niceTicks(980, 1010), [980, 990, 1000, 1010]);
  assert.deepEqual(niceTicks(0, 0), [0, 1]);
  assert.deepEqual(niceTicks(0, 1, 2), [0, 0.5, 1]);
  assert.equal(axisFloor(4, 38), 0);
  assert.equal(axisFloor(980, 1010), 980);
  assert.equal(axisFloor(-5, 10), -5);
});

test("dates read short; other times as written", () => {
  const NOW = Date.UTC(2026, 9, 13);
  assert.equal(whenText("2026-10-12", NOW), "Oct 12");
  assert.equal(whenText("2026-10-12T09:30:00Z", NOW), "Oct 12, 09:30");
  assert.equal(whenText("2025-03-01", NOW), "Mar 1 2025");
  assert.equal(whenText("yesterday", NOW), "yesterday");
});

test("charts describe themselves in one line", () => {
  assert.equal(describe(ok({ type: "bars", title: "PRs", data: { parzi: 4, thesis: 1 } })), "PRs: bar chart, parzi 4, thesis 1");
  assert.equal(describe(ok({ type: "line", data: [3, 9, 4], unit: "ms" })), "line chart, one series over 3 points, from 3 ms to 9 ms");
});
