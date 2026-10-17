import { test } from "node:test";
import assert from "node:assert/strict";
import {
  addQuote,
  deriveItems,
  groupItems,
  itemState,
  linkedByItem,
  MAX_QUOTES,
  progress,
  quoteBlock,
  runTrouble,
  sessionTone,
  splitQuotes,
  type StateFacts,
} from "../src/lib/planModel.ts";
import type { Plan, PlanItem, PrInfo, RunOutcome, SessionMeta } from "../src/lib/api.ts";

const NOW = 1_800_000_000_000;

function item(id: string, over: Partial<PlanItem> = {}): PlanItem {
  return { id, title: id.toUpperCase(), detail: "", phase: "", needs: [], state: "planned", branch: "", note: "", ...over };
}

function plan(items: PlanItem[], phases: string[] = []): Plan {
  return { version: 1, title: "0.1.28", goal: "", phases, items, updated: NOW, by: "you" };
}

function session(id: string, over: Partial<SessionMeta> = {}): SessionMeta {
  return {
    id,
    title: id,
    pinned: false,
    project: "parzi",
    lane: "build",
    model: "claude/claude-opus-5-5",
    status: "idle",
    tokens_in: 0,
    tokens_out: 0,
    cost_usd: 0,
    cwd: "D:\\Repos\\Parzi",
    parent_id: null,
    created: new Date(NOW - 3_600_000).toISOString(),
    updated: new Date(NOW - 60_000).toISOString(),
    plan_item: null,
    branch: "",
    last_run: null,
    ...over,
  };
}

function run(over: Partial<RunOutcome> = {}): RunOutcome {
  return { end: "completed", summary: "", files: [], files_more: 0, checks: [], cost_usd: null, ms: 1000, ended: NOW - 60_000, ...over };
}

function pr(number: number, head: string, over: Partial<PrInfo> = {}): PrInfo {
  return { number, title: `PR ${number}`, url: `https://github.com/o/r/pull/${number}`, head, state: "open", draft: false, checks: "pass", ...over };
}

const none: StateFacts = { merged: false, asks: false, trouble: false, working: false, review: false, touched: false };

function derive(p: Plan, over: Partial<Parameters<typeof deriveItems>[1]> = {}) {
  return deriveItems(p, { project: "parzi", threads: [], running: new Set(), waiting: new Map(), prs: [], activity: new Map(), ...over });
}

test("state precedence: done, dropped, needs you, working, in review, started, not started", () => {
  const all: StateFacts = { merged: false, asks: true, trouble: true, working: true, review: true, touched: true };
  assert.equal(itemState({ state: "done" }, none), "done");
  assert.equal(itemState({ state: "dropped" }, { ...all, merged: true }), "done");
  assert.equal(itemState({ state: "dropped" }, all), "dropped");
  assert.equal(itemState({ state: "active" }, all), "needs");
  assert.equal(itemState({ state: "active" }, { ...all, asks: false }), "needs");
  assert.equal(itemState({ state: "active" }, { ...all, asks: false, trouble: false }), "working");
  assert.equal(itemState({ state: "active" }, { ...none, review: true, touched: true }), "review");
  assert.equal(itemState({ state: "active" }, none), "started");
  assert.equal(itemState({ state: "planned" }, { ...none, touched: true }), "started");
  assert.equal(itemState({ state: "planned" }, none), "todo");
});

test("an item's sessions: the ones naming it in this project, and their helpers", () => {
  const threads = [
    session("a", { plan_item: "names" }),
    session("a1", { parent_id: "a" }),
    session("a2", { parent_id: "a1" }),
    session("a3", { parent_id: "a", plan_item: "other" }),
    session("b", { plan_item: "names", project: "thesis" }),
    session("c"),
  ];
  const linked = linkedByItem(threads, "Parzi");
  assert.deepEqual(linked.get("names")?.map((s) => s.id), ["a", "a1", "a2"]);
  assert.deepEqual(linked.get("other")?.map((s) => s.id), ["a3"]);
  assert.equal(linkedByItem(threads, "thesis").get("names")?.[0].id, "b");
});

test("a merged PR on the item's branch makes it done; an open one puts it in review", () => {
  const p = plan([item("x", { state: "active", branch: "plan/x" }), item("y"), item("z", { branch: "main" })]);
  const threads = [session("s", { plan_item: "y", branch: "plan/y" }), session("t", { plan_item: "z", branch: "main" })];
  const prs = [pr(40, "plan/x", { state: "merged" }), pr(42, "plan/y", { checks: "fail" }), pr(7, "main"), pr(41, "plan/y", { state: "closed" })];
  const [x, y, z] = derive(p, { threads, prs });
  assert.equal(x.state, "done");
  assert.equal(x.pr?.number, 40);
  assert.equal(y.state, "review");
  assert.equal(y.pr?.number, 42);
  assert.deepEqual(y.branches, ["plan/y"]);
  // main says nothing about an item.
  assert.equal(z.state, "started");
  assert.equal(z.pr, null);
});

test("needs you: an open approval, a failed run or a failed check, unless a run goes on", () => {
  const p = plan([item("a", { state: "active" }), item("b", { state: "active" }), item("c", { state: "active" }), item("d", { state: "active" })]);
  const threads = [
    session("sa", { plan_item: "a", status: "active" }),
    session("sb", { plan_item: "b", last_run: run({ end: "failed" }) }),
    session("sc", { plan_item: "c", last_run: run({ checks: [{ cmd: "npm test", ok: false }, { cmd: "cargo test", ok: true }] }) }),
    session("sd", { plan_item: "d", last_run: run({ end: "failed", ended: NOW - 9_000 }) }),
    session("sd2", { plan_item: "d", status: "active" }),
  ];
  const [a, b, c, d] = derive(p, { threads, waiting: new Map([["sa", "Waiting for your approval"]]) });
  assert.equal(a.state, "needs");
  assert.equal(a.reason, "Waiting for your approval");
  assert.equal(b.state, "needs");
  assert.equal(b.reason, "The last run failed");
  assert.equal(c.state, "needs");
  assert.equal(c.reason, "npm test failed");
  assert.equal(d.state, "working");
});

test("a later good run clears an earlier failure", () => {
  const p = plan([item("a", { state: "active" })]);
  const threads = [
    session("s1", { plan_item: "a", last_run: run({ end: "failed", ended: NOW - 50_000 }) }),
    session("s2", { plan_item: "a", last_run: run({ ended: NOW - 10_000 }) }),
  ];
  assert.equal(derive(p, { threads })[0].state, "started");
});

test("working items say what their agents do and how many run", () => {
  const p = plan([item("a", { state: "active" })]);
  const threads = [session("s", { plan_item: "a", status: "active" }), session("k", { parent_id: "s" })];
  const activity = new Map([
    ["s", { label: "Reading notes.md", at: 1 }],
    ["k", { label: "Editing ui/src/lib/SidePanel.svelte", at: 2 }],
  ]);
  const [a] = derive(p, { threads, running: new Set(["k"]), activity });
  assert.equal(a.state, "working");
  assert.equal(a.agents, 2);
  assert.equal(a.doing, "Editing ui/src/lib/SidePanel.svelte");
});

test("open needs read as after, done and dropped ones don't", () => {
  const p = plan([item("a", { state: "done" }), item("b", { state: "dropped" }), item("c", { state: "active" }), item("d", { needs: ["a", "b", "c", "d", "zz"] })]);
  const d = derive(p)[3];
  assert.equal(d.state, "todo");
  assert.deepEqual(d.after, ["C"]);
});

test("groups follow the phases, closed items apart, and progress leaves dropped out", () => {
  const p = plan(
    [item("a", { phase: "Next" }), item("b"), item("c", { phase: "Now", state: "done" }), item("d", { phase: "Now", state: "dropped" }), item("e", { phase: "Later" })],
    ["Now", "Next"],
  );
  const views = derive(p);
  const g = groupItems(p, views);
  assert.deepEqual(
    g.open.map((x) => [x.phase, x.items.map((v) => v.item.id)]),
    [
      ["Now", ["b"]],
      ["Next", ["a"]],
      ["Later", ["e"]],
    ],
  );
  assert.deepEqual(
    g.closed.map((v) => v.item.id),
    ["c", "d"],
  );
  assert.deepEqual(progress(views), { done: 1, total: 4 });
  assert.deepEqual(groupItems(plan([item("a")]), derive(plan([item("a")]))).open[0].phase, "");
});

test("session dots: needs you, running, failed, done", () => {
  const running = new Set(["r"]);
  assert.equal(sessionTone(session("w"), running, new Set(["w"])), "need");
  assert.equal(sessionTone(session("r"), running), "live");
  assert.equal(sessionTone(session("x", { status: "active" }), new Set()), "live");
  assert.equal(sessionTone(session("f", { last_run: run({ end: "crashed" }) }), running), "bad");
  assert.equal(sessionTone(session("p", { last_run: run({ end: "paused" }) }), running), "need");
  assert.equal(sessionTone(session("c", { last_run: run({ checks: [{ cmd: "npm test", ok: false }] }) }), running), "need");
  assert.equal(sessionTone(session("d", { last_run: run() }), running), "ok");
  assert.equal(sessionTone(session("i"), running), "idle");
  assert.equal(runTrouble(run({ checks: [{ cmd: "a", ok: false }, { cmd: "b", ok: false }] })), "a and 1 more failed");
});

test("quote blocks: the thread hides them and shows what they quote", () => {
  const sent = `Start this, keep it small.\n\n<plan-item project="parzi" id="names" title="Session &quot;names&quot;">\nName sessions in 3 to 5 words.\nState: active. Needs: none.\n</plan-item>\n<plan-item project="parzi" id="panel" title="Plan panel">\n</plan-item>`;
  assert.deepEqual(splitQuotes(sent), {
    text: "Start this, keep it small.",
    quotes: [
      { project: "parzi", id: "names", title: 'Session "names"' },
      { project: "parzi", id: "panel", title: "Plan panel" },
    ],
  });
  assert.deepEqual(splitQuotes(quoteBlock({ project: "p", id: "a", title: 'A "b" & c' })), { text: "", quotes: [{ project: "p", id: "a", title: 'A "b" & c' }] });
  assert.deepEqual(splitQuotes("No quotes <b>here</b>"), { text: "No quotes <b>here</b>", quotes: [] });
});

test("the composer quotes an item once, and at most four", () => {
  let list = addQuote([], { project: "parzi", id: "a", title: "A" });
  assert.equal(addQuote(list, { project: "Parzi", id: "a", title: "A" }), list);
  for (const id of ["b", "c", "d"]) list = addQuote(list, { project: "parzi", id, title: id });
  assert.equal(list.length, MAX_QUOTES);
  assert.equal(addQuote(list, { project: "parzi", id: "e", title: "E" }), list);
});
