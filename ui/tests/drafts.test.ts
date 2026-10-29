import { test } from "node:test";
import assert from "node:assert/strict";
import { draftKey, sameDraft, withDraft, type Drafts } from "../src/lib/drafts.ts";

const d = (input: string, extra: Partial<{ quotes: { id: string; project: string; title: string }[]; attachments: string[] }> = {}) => ({
  input,
  quotes: (extra.quotes ?? []) as never[],
  attachments: extra.attachments ?? [],
});

test("a session's draft is its own; a new session's belongs to its tab", () => {
  assert.equal(draftKey({ id: "tab-1", sessionId: "abc" }), "s:abc");
  assert.equal(draftKey({ id: "tab-1", sessionId: null }), "t:tab-1");
});

test("typing keeps it, clearing (or sending) drops it", () => {
  let all: Drafts = {};
  all = withDraft(all, "s:a", d("half a thought"), 1);
  assert.equal(all["s:a"].input, "half a thought");
  all = withDraft(all, "s:a", d("  "), 2);
  assert.equal(all["s:a"], undefined);
  all = withDraft(all, "s:b", d("", { attachments: ["C:/x.png"] }), 3);
  assert.deepEqual(all["s:b"].attachments, ["C:/x.png"]);
});

test("only the newest sixty are kept", () => {
  let all: Drafts = {};
  for (let i = 0; i < 65; i++) all = withDraft(all, `s:${i}`, d(`draft ${i}`), i);
  assert.equal(Object.keys(all).length, 60);
  assert.equal(all["s:0"], undefined);
  assert.equal(all["s:64"].input, "draft 64");
});

test("the same text is not a change", () => {
  const all = withDraft({}, "s:a", d("x", { attachments: ["f"] }), 1);
  assert.ok(sameDraft(all["s:a"], d("x", { attachments: ["f"] })));
  assert.ok(!sameDraft(all["s:a"], d("xy", { attachments: ["f"] })));
  assert.ok(sameDraft(undefined, d("")));
});
