// Run: node --test ui/tests/deckState.test.ts   (node >= 23 strips the types)
// The deck's cards are derived, never guessed: same PLAN.md + journal.jsonl,
// same states. Lives outside ui/src so svelte-check (include: src/**/*)
// stays node-free.
import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import {
  JOURNAL_KINDS,
  PROJECT_STATUSES,
  type JournalLine,
  type Plan,
  type Project,
} from "../src/lib/api.ts";
import {
  etagOf,
  laneNames,
  liveLaneCount,
  projectMarkdown,
  sprintPosition,
  taskStates,
} from "../src/lib/deck/derive.ts";

function task(id: string, over: Record<string, unknown> = {}) {
  return {
    id,
    title: `task ${id}`,
    repo: "shop-api",
    scope: ["src/**"],
    after: [],
    critical: false,
    done: false,
    acceptance: [],
    ...over,
  };
}

const PLAN: Plan = {
  sprints: [
    {
      title: "Sprint 1",
      target: "1 day",
      lanes: [
        { name: "api", tasks: [task("TSK-7"), task("TSK-8", { critical: true })] },
        { name: "web", tasks: [task("TSK-9", { repo: "shop-web", after: ["TSK-7"] })] },
      ],
    },
    {
      title: "Sprint 2",
      target: "",
      lanes: [{ name: "api", tasks: [task("TSK-11")] }],
    },
  ],
} as Plan;

function line(kind: JournalLine["kind"], task: string | null, who = "lane api", text = ""): JournalLine {
  return { at: "2026-09-16T09:00:00Z", who, kind, task, text };
}

test("an untouched plan is all pending", () => {
  const live = taskStates(PLAN, []);
  assert.deepEqual(Object.keys(live).sort(), ["TSK-11", "TSK-7", "TSK-8", "TSK-9"]);
  assert.equal(live["TSK-7"].state, "pending");
  assert.equal(liveLaneCount(live), 0);
});

test("claim then release leaves the task pending again", () => {
  const live = taskStates(PLAN, [line("claim", "TSK-7"), line("release", "TSK-7")]);
  assert.equal(live["TSK-7"].state, "pending");
  assert.equal(live["TSK-7"].holder, "");
});

test("a heartbeat Note turns a claim into running with tool and turn", () => {
  const live = taskStates(PLAN, [
    line("claim", "TSK-8"),
    line("note", "TSK-8", "lane api", "tool:edit turn:6 on src/payments/stripe.rs"),
  ]);
  assert.equal(live["TSK-8"].state, "running");
  assert.equal(live["TSK-8"].tool, "edit");
  assert.equal(live["TSK-8"].turn, 6);
  assert.equal(live["TSK-8"].holder, "lane api");
  assert.equal(liveLaneCount(live), 1);
});

test("a handoff beats a later heartbeat and clears a pending request", () => {
  const live = taskStates(PLAN, [
    line("claim", "TSK-7"),
    line("request", "TSK-7"),
    line("handoff", "TSK-7", "lane api", "capsule written"),
    line("note", "TSK-7", "lane api", "tool:read turn:9"),
  ]);
  assert.equal(live["TSK-7"].state, "done");
  assert.equal(live["TSK-7"].requestPending, false);
});

test("request sets the pending pill, grant and deny clear it", () => {
  const asked = taskStates(PLAN, [line("request", "TSK-9", "lane web")]);
  assert.equal(asked["TSK-9"].requestPending, true);
  const granted = taskStates(PLAN, [line("request", "TSK-9", "lane web"), line("grant", "TSK-9")]);
  assert.equal(granted["TSK-9"].requestPending, false);
  const denied = taskStates(PLAN, [line("request", "TSK-9", "lane web"), line("deny", "TSK-9")]);
  assert.equal(denied["TSK-9"].requestPending, false);
});

test("a block wins over a claim and shows on the card", () => {
  const live = taskStates(PLAN, [line("claim", "TSK-9", "lane web"), line("block", "TSK-9", "lane web", "waiting on routes.rs")]);
  assert.equal(live["TSK-9"].state, "blocked");
  assert.equal(live["TSK-9"].note, "waiting on routes.rs");
});

test("lane columns keep first-seen order across sprints", () => {
  assert.deepEqual(laneNames(PLAN), ["api", "web"]);
});

test("sprint n/m is the first sprint with unfinished work", () => {
  assert.deepEqual(sprintPosition(PLAN, taskStates(PLAN, [])), { n: 1, m: 2 });
  const sprint1Done = taskStates(PLAN, [
    line("handoff", "TSK-7"),
    line("handoff", "TSK-8"),
    line("handoff", "TSK-9", "lane web"),
  ]);
  assert.deepEqual(sprintPosition(PLAN, sprint1Done), { n: 2, m: 2 });
});

test("etag changes with the content and prefers a command's own etag", () => {
  const a = etagOf([line("claim", "TSK-7")]);
  assert.equal(a, etagOf([line("claim", "TSK-7")]));
  assert.notEqual(a, etagOf([line("claim", "TSK-8")]));
  assert.equal(etagOf({ text: "anything", etag: "mtime-42" }), "mtime-42");
});

test("PROJECT.md round-trips the fields the grammar names", () => {
  const p: Project = {
    slug: "checkout-flow",
    title: "Checkout flow",
    workspace: "acme",
    repos: ["shop-api", "shop-web"],
    roster: { header: "google/gemini-2.5-flash", orchestrator: "anthropic/claude-fable-5-1", coder: "opencode/muse-spark-1.3" },
    budget_usd: 40,
    status: "running",
    critical: ["shop-api/src/payments/**"],
    why: "Card checkout bounces people to a hosted page.",
    what: [{ text: "one page checkout", done: true }, { text: "idempotent intents", done: false }],
    constraints: ["no schema migrations this sprint"],
  };
  const md = projectMarkdown(p);
  assert.match(md, /^# Project: Checkout flow\n/);
  assert.match(md, /roster: header = google\/gemini-2\.5-flash, orchestrator = anthropic\/claude-fable-5-1, coder = opencode\/muse-spark-1\.3/);
  assert.match(md, /\ncritical: shop-api\/src\/payments\/\*\*\n/);
  assert.match(md, /- \[x\] one page checkout\n- \[ \] idempotent intents/);
  assert.match(md, /## Constraints\n\n- no schema migrations this sprint/);
  // The pills above the document already say these; repeating them is what
  // made the header block a paragraph of run-together prose.
  assert.doesNotMatch(md, /\nworkspace: /);
  assert.doesNotMatch(md, /\nrepos: /);
  assert.doesNotMatch(md, /\nbudget: /);
  assert.doesNotMatch(md, /\nstatus: /);
});

test("the collapsed PROJECT.md is Why and What, and nothing else", () => {
  const p: Project = {
    slug: "checkout-flow",
    title: "Checkout flow",
    workspace: "acme",
    repos: ["shop-api"],
    roster: { header: "", orchestrator: "", coder: "" },
    budget_usd: null,
    status: "drafting",
    critical: [],
    why: "Card checkout bounces people to a hosted page.",
    what: [{ text: "one page checkout", done: true }],
    constraints: ["no schema migrations this sprint"],
  };
  const brief = projectMarkdown(p, false);
  assert.match(brief, /^## Why\n/);
  assert.match(brief, /## What\n\n- \[x\] one page checkout/);
  assert.doesNotMatch(brief, /# Project:/);
  assert.doesNotMatch(brief, /roster:/);
  assert.doesNotMatch(brief, /## Constraints/);
  // An empty roster still reads as a roster row in the full document.
  assert.match(projectMarkdown(p), /roster: header = auto, orchestrator = auto, coder = auto/);
  assert.match(projectMarkdown(p), /\ncritical: nothing marked\n/);
});

test("the status and journal unions are the words parzi-core puts on the wire", async () => {
  // Hand-transcribed from crates/parzi-core/src/{project/mod.rs,journal.rs};
  // both enums rename their variants, so a capitalised union never matches.
  const wire = JSON.parse(
    await readFile(new URL("./fixtures/project-status.json", import.meta.url), "utf8"),
  ) as { statuses: string[]; default: string; journal_kinds: string[] };

  assert.deepEqual([...PROJECT_STATUSES], wire.statuses);
  assert.ok(PROJECT_STATUSES.includes(wire.default as (typeof PROJECT_STATUSES)[number]));
  assert.deepEqual([...JOURNAL_KINDS], wire.journal_kinds);
  for (const s of [...PROJECT_STATUSES, ...JOURNAL_KINDS]) {
    assert.match(s, /^[a-z][a-z_]*$/, `${s} is not what serde writes`);
  }
});
