import { test } from "node:test";
import assert from "node:assert/strict";
import { folderName, hostOf, isExplicitUrl, restoreComposer, restoreTab, toAddress } from "../src/lib/tabs.ts";
import { normLane } from "../src/lib/lanes.ts";

test("hostOf strips protocol and path", () => {
  assert.equal(hostOf("https://github.com/lucas/parzi"), "github.com");
  assert.equal(hostOf("https://www.example.com/a?b#c"), "example.com");
  assert.equal(hostOf(""), "");
});

test("toAddress builds https or search fallback", () => {
  assert.equal(toAddress("https://example.com/x"), "https://example.com/x");
  assert.equal(toAddress("example.com"), "https://example.com");
  assert.equal(toAddress("hello world"), "https://duckduckgo.com/?q=hello%20world");
});

test("isExplicitUrl is strict", () => {
  assert.equal(isExplicitUrl("https://example.com/x"), true);
  assert.equal(isExplicitUrl("example.com"), false);
  assert.equal(isExplicitUrl("hello world"), false);
});

test("folderName takes the leaf", () => {
  assert.equal(folderName("C:\\src\\parzi"), "parzi");
  assert.equal(folderName("/home/u/app/"), "app");
});

test("legacy lanes normalize", () => {
  assert.equal(normLane("code"), "build");
  assert.equal(normLane("research"), "work");
  assert.equal(normLane("build"), "build");
  assert.equal(normLane(""), "");
});

test("saved Work and Tasks tabs don't come back", () => {
  assert.equal(restoreTab({ id: "t1", kind: "tasks", title: "Tasks" }), null);
  assert.equal(restoreTab({ id: "t2", kind: "work", title: "Work" }), null);
  assert.deepEqual(restoreTab({ id: "t5", kind: "brain", title: "Brain" }), { id: "t5", kind: "brain", title: "Brain" });
  assert.equal(restoreTab({ id: "t3", kind: "settings", title: "Settings" }), null);
  assert.equal(restoreTab({ id: "t4", kind: "mystery", title: "?" }), null);
});

test("a saved composer comes back whole, whatever an older build saved", () => {
  assert.deepEqual(restoreComposer(undefined), { input: "", mode: "build", model: "", effort: "medium", attachments: [] });
  // Older names for the modes, "auto" for no model, junk in the lists.
  assert.deepEqual(restoreComposer({ mode: "research", model: "auto", attachments: ["a.png", 4], draft: "hi" }), {
    input: "",
    mode: "work",
    model: "",
    effort: "medium",
    attachments: ["a.png"],
  });
  assert.equal(restoreComposer({ mode: "code" }).mode, "build");
  assert.equal(restoreComposer({ mode: "chat" }).mode, "build");
  assert.deepEqual(restoreComposer({ input: "x", mode: "search", model: "codex/gpt-5", effort: "high", attachments: [], remote: true, remoteCwd: "/srv" }), {
    input: "x",
    mode: "search",
    model: "codex/gpt-5",
    effort: "high",
    attachments: [],
    remote: true,
    remoteCwd: "/srv",
  });
});
