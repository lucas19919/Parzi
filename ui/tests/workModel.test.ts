import { test } from "node:test";
import assert from "node:assert/strict";
import {
  ALL,
  DAY_MS,
  WEEK_MS,
  bandOf,
  basename,
  cardLine,
  clip,
  compactRail,
  engineLabel,
  engineMatch,
  firstLine,
  formatCost,
  formatElapsed,
  goalActivity,
  groupBands,
  homeSummary,
  inProject,
  otherEngines,
  placeOf,
  projectOptions,
  railLayout,
  runTime,
  runTone,
  runWord,
  railOrder,
  shortLabel,
  statusWord,
  stepProgress,
  stepTone,
  stepWord,
  timeOf,
  toneOf,
  waitsOn,
  whenOf,
} from "../src/lib/workModel.ts";
import type { GoalView, RunView, StepView } from "../src/lib/api.ts";

const NOW = 1_800_000_000_000;

function goal(id: string, over: Partial<GoalView> = {}): GoalView {
  return {
    id,
    title: id.toUpperCase(),
    status: "working",
    project: "parzi",
    root: `root-${id}`,
    prompt: "",
    created: NOW - 3_600_000,
    updated: NOW - 60_000,
    engine: { kind: "here", name: "This PC" },
    steps: [],
    running: [],
    last: null,
    cost_usd: null,
    files: 0,
    asks: 0,
    later: null,
    owned: true,
    ...over,
  };
}

function step(key: string, over: Partial<StepView> = {}): StepView {
  return {
    key,
    title: `Step ${key}`,
    needs: [],
    lane: "build",
    status: "todo",
    note: null,
    outcome: null,
    sessions: [],
    auto: false,
    ready: true,
    blocked_by: [],
    ...over,
  };
}

function run(over: Partial<RunView> = {}): RunView {
  return {
    id: "r1",
    session: "s1",
    step: null,
    model: "claude/claude-opus-5-5",
    lane: "build",
    cause: "send",
    started: NOW - 240_000,
    ended: null,
    end: null,
    error: null,
    ms: 0,
    turns: 0,
    tokens_in: 0,
    tokens_out: 0,
    cost_usd: null,
    summary: "",
    files: [],
    files_more: 0,
    checks: [],
    artifacts: 0,
    ...over,
  };
}

// a -> b -> c, a -> d: one done, one running, one blocked, one ready.
const PLAN = [
  step("a", { status: "done", sessions: ["s5"] }),
  step("b", { status: "running", needs: ["a"], sessions: ["s6"] }),
  step("c", { needs: ["b"], ready: false, blocked_by: ["b"] }),
  step("d", { needs: ["a"] }),
];

test("goals land in the band for what they need", () => {
  for (const status of ["needs_you", "paused", "check", "failed"] as const) assert.equal(bandOf(goal("g", { status }), [], NOW), "needs");
  assert.equal(bandOf(goal("g"), [], NOW), "working");
  assert.equal(bandOf(goal("g", { status: "later", later: { auto_start: false } }), [], NOW), "later");
  for (const status of ["done", "stopped", "cut_off"] as const) assert.equal(bandOf(goal("g", { status }), [], NOW), "done");
  assert.equal(bandOf(goal("g", { status: "archived" }), [], NOW), null);
});

test("an open ask puts working goals in Needs you", () => {
  assert.equal(bandOf(goal("g", { asks: 1 }), [], NOW), "needs");
  const ask = { id: "k", session: "s", goal: "g", kind: "approval" as const, label: "", detail: null, opened: NOW, expires: null };
  assert.equal(bandOf(goal("g"), [ask], NOW), "needs");
  assert.equal(bandOf(goal("h"), [ask], NOW), "working");
});

test("finished goals leave Done recently after a week", () => {
  assert.equal(bandOf(goal("g", { status: "done", updated: NOW - WEEK_MS + 1000 }), [], NOW), "done");
  assert.equal(bandOf(goal("g", { status: "done", updated: NOW - WEEK_MS - 1000 }), [], NOW), null);
});

test("bands keep a steady order", () => {
  const b = groupBands(
    [
      goal("w-old", { created: NOW - 9000, updated: NOW }),
      goal("w-new", { created: NOW - 1000, updated: NOW - 5000 }),
      goal("l2", { status: "later", created: NOW - 1000 }),
      goal("l1", { status: "later", created: NOW - 2000 }),
      goal("d1", { status: "done", updated: NOW - 2000 }),
      goal("d2", { status: "stopped", updated: NOW - 1000 }),
    ],
    [],
    NOW,
  );
  // An update (a ledger append) does not move a working goal up.
  assert.deepEqual(b.working.map((g) => g.id), ["w-new", "w-old"]);
  assert.deepEqual(b.later.map((g) => g.id), ["l1", "l2"]);
  assert.deepEqual(b.done.map((g) => g.id), ["d2", "d1"]);
});

test("status words", () => {
  assert.equal(statusWord(goal("g")), "Working");
  assert.equal(statusWord(goal("g", { asks: 2 })), "Waiting for you");
  assert.equal(statusWord(goal("g", { status: "paused" })), "Paused: budget");
  assert.equal(statusWord(goal("g", { status: "check" })), "Check this");
  assert.equal(statusWord(goal("g", { status: "failed" })), "Didn't finish");
  assert.equal(statusWord(goal("g", { status: "cut_off" })), "Cut off");
  assert.equal(statusWord(goal("g", { status: "later", later: { auto_start: false } })), "Later");
  assert.equal(statusWord(goal("g", { status: "later", later: { auto_start: true } })), "Starts when free");
  assert.equal(toneOf(goal("g")), "live");
  assert.equal(toneOf(goal("g", { status: "check" })), "need");
  assert.equal(toneOf(goal("g", { status: "failed" })), "bad");
  assert.equal(toneOf(goal("g", { status: "stopped" })), "idle");
});

test("step words say when a step waits on another", () => {
  assert.equal(stepWord(PLAN[0]), "Done");
  assert.equal(stepWord(PLAN[1]), "Working");
  assert.equal(stepWord(PLAN[2]), "Waiting");
  assert.equal(stepWord(PLAN[3]), "To do");
  assert.equal(waitsOn(PLAN[2], PLAN), "Waits on Step b");
  assert.equal(waitsOn(step("x", { blocked_by: ["a", "b"] }), PLAN), "Waits on Step a and 1 more");
  assert.equal(waitsOn(PLAN[3], PLAN), "");
});

test("the rail orders steps by depth, then as planned", () => {
  assert.deepEqual(
    railOrder(PLAN).map((o) => [o.step.key, o.layer]),
    [
      ["a", 0],
      ["b", 1],
      ["d", 1],
      ["c", 2],
    ],
  );
});

test("rail depth survives a cycle and unknown needs", () => {
  const steps = [step("a", { needs: ["b", "zz"] }), step("b", { needs: ["a"] })];
  const order = railOrder(steps);
  assert.equal(order.length, 2);
  const layout = railLayout(steps);
  // The back edge of the cycle is dropped, the other edge drawn.
  assert.equal(layout.rows.reduce((n, r) => n + r.out.length, 0), 1);
});

test("the vertical rail draws every need as a line down to its step", () => {
  const { rows, cols } = railLayout(PLAN);
  assert.equal(cols, 2);
  const at = (k: string) => rows.find((r) => r.step.key === k)!;
  // a fans out to b (same column) and d (a new column).
  assert.equal(at("a").col, 0);
  assert.deepEqual(at("a").out, [
    { col: 0, met: true },
    { col: 1, met: true },
  ]);
  assert.equal(at("b").col, 0);
  assert.deepEqual(at("b").into, [{ col: 0, met: true }]);
  // The line to d passes b's row; b's line to c is not met yet.
  assert.deepEqual(at("b").through, [{ col: 1, met: true }]);
  assert.deepEqual(at("b").out, [{ col: 0, met: false }]);
  assert.equal(at("d").col, 1);
  assert.deepEqual(at("d").through, [{ col: 0, met: false }]);
  assert.equal(at("c").col, 0);
  assert.deepEqual(at("c").into, [{ col: 0, met: false }]);
});

test("a step with two needs merges their lines", () => {
  const steps = [step("a"), step("b"), step("c", { needs: ["a", "b"] })];
  const { rows, cols } = railLayout(steps);
  assert.equal(cols, 2);
  assert.deepEqual(rows.map((r) => r.col), [0, 1, 0]);
  assert.deepEqual(rows[2].into.map((l) => l.col), [0, 1]);
});

test("a plan without needs is one column of dots", () => {
  const { rows, cols } = railLayout([step("a"), step("b"), step("c")]);
  assert.equal(cols, 1);
  assert.ok(rows.every((r) => r.col === 0 && !r.into.length && !r.out.length));
});

test("the compact rail joins stages and stops at twelve", () => {
  const r = compactRail(PLAN);
  assert.deepEqual(r.dots.map((d) => [d.key, d.next]), [
    ["a", ""],
    ["b", "line"],
    ["d", "side"],
    ["c", "line"],
  ]);
  const many = Array.from({ length: 15 }, (_, i) => step(`s${i}`));
  const big = compactRail(many);
  assert.equal(big.dots.length, 12);
  assert.equal(big.more, 3);
});

test("progress points at the running step, else the next to look at", () => {
  assert.deepEqual(
    (({ done, total, current, index }) => ({ done, total, key: current?.key, index }))(stepProgress(PLAN)),
    { done: 1, total: 4, key: "b", index: 2 },
  );
  const all = stepProgress([step("a", { status: "done" }), step("b", { status: "skipped" })]);
  assert.equal(all.current, null);
  assert.equal(all.done, 2);
  assert.equal(stepProgress([step("a", { status: "done" }), step("b", { status: "check" }), step("c")]).current?.key, "b");
});

test("a session finds its goal and step", () => {
  const goals = [goal("plan", { root: "s1", steps: PLAN, running: ["s6", "s9"] }), goal("other", { root: "s2" })];
  const root = placeOf(goals, "s1");
  assert.equal(root?.goal.id, "plan");
  assert.equal(root?.root, true);
  assert.equal(root?.step?.key, "b");
  assert.equal(root?.index, 2);
  const child = placeOf(goals, "s5");
  assert.equal(child?.root, false);
  assert.equal(child?.step?.key, "a");
  assert.equal(child?.index, 1);
  assert.equal(child?.total, 4);
  // Spawned without a step, while it runs.
  const helper = placeOf(goals, "s9");
  assert.equal(helper?.goal.id, "plan");
  assert.equal(helper?.step, null);
  assert.equal(placeOf(goals, "s2")?.goal.id, "other");
  assert.equal(placeOf(goals, "nope"), null);
  assert.equal(placeOf(goals, null), null);
});

test("activity lines name the file, not its path", () => {
  assert.equal(shortLabel("Editing D:\\Repos\\Parzi\\ui\\src\\lib\\Omnibar.svelte"), "Editing Omnibar.svelte");
  assert.equal(shortLabel("Reading crates/parzi-core/src/sync.rs"), "Reading sync.rs");
  assert.equal(shortLabel("Running `cargo test -p parzi-core`"), "Running cargo test -p parzi-core");
  assert.equal(shortLabel("Opening https://docs.rs/tauri"), "Opening https://docs.rs/tauri");
  assert.equal(basename("C:\\src\\app\\"), "app");
});

test("a goal's activity is its newest running session's", () => {
  const act = new Map([
    ["s6", { label: "Editing ui/src/lib/Omnibar.svelte", at: 20 }],
    ["s7", { label: "Reading notes.md", at: 10 }],
    ["s8", { label: "Old", at: 99 }],
  ]);
  assert.deepEqual(goalActivity({ running: ["s6", "s7"] }, act), { text: "Editing Omnibar.svelte", more: 1 });
  assert.equal(goalActivity({ running: ["s1"] }, act), null);
  assert.equal(goalActivity({ running: [] }, act), null);
});

test("first lines drop markdown", () => {
  assert.equal(firstLine("\n## Fixed the **freeze**\nmore"), "Fixed the freeze");
  assert.equal(firstLine("- `npm test` passes"), "npm test passes");
  assert.equal(firstLine(""), "");
  assert.equal(firstLine(null), "");
});

test("elapsed and cost read short", () => {
  assert.equal(formatElapsed(42_000), "42s");
  assert.equal(formatElapsed(4 * 60_000 + 10_000), "4m");
  assert.equal(formatElapsed(65 * 60_000), "1h 5m");
  assert.equal(formatElapsed(2 * 3_600_000), "2h");
  assert.equal(formatElapsed(DAY_MS + 3_600_000), "1d 1h");
  assert.equal(formatElapsed(-5), "0s");
  assert.equal(formatCost(null), "");
  assert.equal(formatCost(0), "");
  assert.equal(formatCost(0.004), "<$0.01");
  assert.equal(formatCost(0.4213), "$0.42");
  assert.equal(formatCost(123.6), "$124");
});

test("a card's time follows the goal", () => {
  assert.equal(timeOf(goal("g", { last: run({ started: NOW - 240_000 }) }), NOW), "4m");
  assert.equal(timeOf(goal("g", { status: "done", updated: NOW - 2 * 3_600_000 }), NOW), "2h ago");
  assert.equal(timeOf(goal("g", { status: "later", created: NOW - 120_000 }), NOW), "Added 2m ago");
});

test("engines and filters", () => {
  assert.equal(engineLabel({ kind: "here", name: "This PC" }), "This PC");
  assert.equal(engineLabel({ kind: "server", name: "box" }), "Server (box)");
  assert.equal(engineLabel({ kind: "device", name: "laptop" }), "laptop");
  const here = goal("h");
  const far = goal("s", { engine: { kind: "server", name: "box" } });
  assert.equal(otherEngines([here]), false);
  assert.equal(otherEngines([here], true), true);
  assert.equal(otherEngines([here, far]), true);
  assert.equal(engineMatch(far, "all"), true);
  assert.equal(engineMatch(far, "here"), false);
  assert.equal(engineMatch(far, "server"), true);
});

test("the project filter lists projects with work", () => {
  const goals = [
    goal("a"),
    goal("b", { project: "thesis", status: "done" }),
    goal("c", { project: null, status: "later" }),
    goal("old", { project: "gone", status: "done", updated: NOW - 2 * WEEK_MS }),
  ];
  const opts = projectOptions(goals, [{ slug: "parzi", title: "Parzi" }], NOW);
  assert.deepEqual(
    opts.map((o) => [o.value, o.label, o.count]),
    [
      [ALL, "All projects", 2],
      ["parzi", "Parzi", 1],
      ["thesis", "thesis", 0],
      ["", "No project", 1],
    ],
  );
  assert.equal(inProject(goals[0], ALL), true);
  assert.equal(inProject(goals[0], "PARZI"), true);
  assert.equal(inProject(goals[2], ""), true);
  assert.equal(inProject(goals[0], ""), false);
});

test("Home sums up what needs you, what runs and what finished today", () => {
  const dayStart = NOW - 3 * 3_600_000;
  const s = homeSummary(
    [
      goal("w"),
      goal("n", { status: "check" }),
      goal("today", { status: "done", updated: NOW - 3_600_000 }),
      goal("yesterday", { status: "done", updated: NOW - 5 * 3_600_000 }),
      goal("stopped", { status: "stopped", updated: NOW - 60_000 }),
    ],
    [],
    NOW,
    dayStart,
  );
  assert.deepEqual(s.needs.map((g) => g.id), ["n"]);
  assert.deepEqual(s.working.map((g) => g.id), ["w"]);
  assert.deepEqual(s.doneToday.map((g) => g.id), ["today"]);
});

test("a card's line says what is happening, else why it waits", () => {
  const act = new Map([["s6", { label: "Editing ui/src/lib/Omnibar.svelte", at: 5 }]]);
  assert.deepEqual(cardLine(goal("g", { running: ["s6"] }), act), { text: "Editing Omnibar.svelte", live: true, more: 0 });
  assert.equal(cardLine(goal("g", { steps: PLAN }), act).text, "Step 2 of 4: Step b");
  const checks = [
    { cmd: "npm test", ok: false },
    { cmd: "cargo test", ok: true },
  ];
  assert.equal(cardLine(goal("g", { status: "check", last: run({ checks }) }), act).text, "npm test failed");
  assert.equal(cardLine(goal("g", { status: "failed", last: run({ error: "3 tool errors in a row\nmore" }) }), act).text, "3 tool errors in a row");
  assert.equal(cardLine(goal("g", { status: "paused", last: run() }), act).text, "Reached its budget");
  assert.equal(cardLine(goal("g", { status: "later", prompt: "Clean up the screenshots\nin docs" }), act).text, "Clean up the screenshots");
  assert.equal(cardLine(goal("g", { status: "done", last: run({ summary: "## Done\nAll good" }) }), act).text, "Done");
  // A goal that waits on you says for what, not its last tool call.
  assert.deepEqual(cardLine(goal("g", { status: "needs_you", running: ["s6"] }), act), { text: "Waiting for your answer in its session", live: false, more: 0 });
  const ask = { id: "k", session: "s6", goal: "g", kind: "approval" as const, label: "Run npm install", detail: null, opened: NOW, expires: null };
  assert.equal(cardLine(goal("g", { running: ["s6"], asks: 1 }), act, [ask]).text, "Run npm install");
});

test("runs say how they ended and how long they took", () => {
  assert.equal(runWord(run()), "Running");
  assert.equal(runTone(run()), "live");
  assert.equal(runTime(run({ started: NOW - 90_000 }), NOW), "1m");
  const done = run({ end: "completed", ended: NOW, ms: 125_000 });
  assert.equal(runWord(done), "Finished");
  assert.equal(runTone(done), "ok");
  assert.equal(runTime(done, NOW), "2m");
  assert.equal(runTone(run({ end: "completed", ended: NOW, checks: [{ cmd: "npm test", ok: false }] })), "need");
  assert.equal(runWord(run({ end: "interrupted", ended: NOW })), "Cut off");
  assert.equal(runTone(run({ end: "crashed", ended: NOW })), "bad");
});

test("clip cuts at a word", () => {
  assert.equal(clip("Editing Omnibar.svelte", 30), "Editing Omnibar.svelte");
  assert.equal(clip("Running cargo test -p parzi-core sync", 24), "Running cargo test -p…");
  assert.equal(clip("Supercalifragilisticexpialidocious", 10), "Supercali…");
});

test("a step's session reads its step's state", () => {
  assert.equal(stepTone(PLAN[0]), "ok");
  assert.equal(stepTone(PLAN[1]), "live");
  assert.equal(stepTone(step("x", { status: "check" })), "need");
  assert.equal(stepTone(step("x", { status: "failed" })), "bad");
  assert.equal(stepTone(PLAN[3]), "idle");
});

test("times today read as a time, older ones with the date", () => {
  const noon = new Date(2026, 9, 12, 12, 0).getTime();
  assert.doesNotMatch(whenOf(noon - 3_600_000, noon), /,/);
  assert.match(whenOf(noon - 2 * DAY_MS, noon), /,/);
});
