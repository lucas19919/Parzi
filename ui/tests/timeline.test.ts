import { test } from "node:test";
import assert from "node:assert/strict";
import type { ChatEvent } from "../src/lib/api.ts";
import { buildTurns, workedLine } from "../src/lib/timeline.ts";
import { describeTool, groupLine } from "../src/lib/toolWords.ts";
import { addCall, addReasoning, addResult, addText, emptyLive, reconcile, reveal, settle } from "../src/lib/liveEvents.ts";

const call = (id: string, name: string, args: unknown = {}): ChatEvent => ({ kind: "tool_call", id, name, args });
const done = (id: string, name: string, output = "ok"): ChatEvent => ({ kind: "tool_result", id, name, ok: true, ms: 10, output });
const say = (text: string, at?: number): ChatEvent => ({ kind: "assistant", text, done: true, at });

test("steps of one kind in a row are one group, narration stays as text", () => {
  const ev: ChatEvent[] = [
    { kind: "user", text: "walk me through it", at: 100 },
    say("I'll follow a message from the composer."),
    call("1", "Read", { file_path: "D:\\a\\composer.rs" }),
    done("1", "Read"),
    { kind: "reasoning", text: "next the shell" },
    call("2", "read_file", { target_file: "D:/a/shell.rs" }),
    done("2", "read_file"),
    call("3", "Grep", { pattern: "emit" }),
    say("Here is how it flows.", 160),
  ];
  const [t] = buildTurns(ev);
  assert.deepEqual(t.blocks.map((b) => b.kind), ["text", "group", "thinking", "group", "text"]);
  const reads = t.blocks[1];
  assert.ok(reads.kind === "group");
  assert.equal(reads.items.length, 2);
  assert.deepEqual(reads.items.map((i) => i.target), ["composer.rs", "shell.rs"]);
  assert.equal(groupLine("read", reads.items, false), "Read 2 files");
  const search = t.blocks[3];
  assert.ok(search.kind === "group" && search.items[0].running);
  assert.equal(workedLine(t), "Worked 1m · 3 steps");
});

test("agents' many names for a step read the same", () => {
  assert.equal(describeTool("Running view_file", { AbsolutePath: "C:\\x\\a.md" }).target, "a.md");
  assert.equal(describeTool("Run edit_file?", { TargetFile: "b.ts" }).kind, "edit");
  assert.equal(describeTool("use_tool", { tool_name: "parzi__session_rename", tool_input: { title: "X" } }).done, "Named the session");
  assert.equal(describeTool("mcp__parzi__fs_read", {}).kind, "read");
  assert.equal(describeTool("shell.exec", { arguments: { cmd: "npm test" } }).done, "Ran tests");
  assert.equal(describeTool("npm run lint", { CommandLine: "npm run lint" }).kind, "run");
  assert.equal(describeTool("github.pull_request_read", {}).done, "Used pull request read");
});

test("live events build the same turn the store does", () => {
  const saved: ChatEvent[] = [
    { kind: "user", text: "go" },
    { kind: "reasoning", text: "thinking" },
    say("Checking."),
    call("1", "Bash", { command: "cargo test" }),
    done("1", "Bash", "test result: ok"),
    say("All good."),
  ];
  let s = emptyLive();
  s = addReasoning(s, "think");
  s = addReasoning(s, "ing");
  s = addText(s, "Check", 5);
  s = addText(s, "ing.", 9);
  s = reveal(s, 3);
  s = addCall(s, "1", "Bash", { command: "cargo test" });
  s = addResult(s, "1", "Bash", true, 10, "test result: ok");
  s = addText(s, "All good.", 9);
  s = settle(s);
  const live = buildTurns([saved[0], ...s.events]);
  const stored = buildTurns(saved);
  const shape = (ts: ReturnType<typeof buildTurns>) => ts[0].blocks.map((b) => [b.key, b.kind, b.kind === "text" || b.kind === "thinking" ? b.text : ""]);
  assert.deepEqual(shape(live), shape(stored));
});

test("text the window already has is not added twice", () => {
  let s = addText(emptyLive(), "Hello", 5);
  s = addText(s, "lo wor", 9);
  assert.equal(s.pending, "Hello wor");
  assert.equal(addText(s, "wor", 9), s);
});

test("a fresh read keeps what came after it and drops what it saved", () => {
  let s = emptyLive();
  s = addCall(s, "1", "Read", { path: "a" });
  s = addResult(s, "1", "Read", true, 1, "x");
  s = addText(s, "Second half", 17);
  const read: ChatEvent[] = [
    { kind: "user", text: "go" },
    call("1", "Read", { path: "a" }),
    done("1", "Read"),
    { kind: "assistant", text: "First ", done: false, unsaved: true, upto: 6 } as ChatEvent,
  ];
  const r = reconcile(read, s);
  assert.equal(r.stored.length, 3);
  assert.deepEqual(r.live.events.map((e) => e.kind), ["assistant"]);
  const all = settle(r.live).events[0];
  assert.ok(all.kind === "assistant");
  assert.equal(all.text, "First Second half");
});
