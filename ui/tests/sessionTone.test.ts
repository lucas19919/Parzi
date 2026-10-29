import { test } from "node:test";
import assert from "node:assert/strict";
import { sessionTone } from "../src/lib/sessionTone.ts";
import { hidePlanItems } from "../src/lib/userText.ts";
import type { RunOutcome, SessionMeta } from "../src/lib/api.ts";

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
    cwd: "D:\Repos\Parzi",
    created: "",
    updated: "",
    ...over,
  };
}

function run(over: Partial<RunOutcome> = {}): RunOutcome {
  return { end: "completed", summary: "", files: [], files_more: 0, checks: [], cost_usd: null, ms: 1000, ended: 0, ...over };
}

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
  assert.equal(sessionTone(session("o", { last_run: { ...run(), checks: undefined as unknown as [] } }), running), "ok");
});

test("older messages' plan items leave the text and show as their titles", () => {
  const sent = 'Start this.\n\n<plan-item project="parzi" id="quotes" title="Quote &quot;chips&quot;">\nClick an item.\n</plan-item>';
  assert.deepEqual(hidePlanItems(sent), { text: "Start this.", items: ['Quote "chips"'] });
  assert.deepEqual(hidePlanItems('<plan-item project="p" id="a">\n</plan-item>'), { text: "", items: ["a"] });
  assert.deepEqual(hidePlanItems("No items <b>here</b>"), { text: "No items <b>here</b>", items: [] });
});
