import { test } from "node:test";
import assert from "node:assert/strict";
import { ensureComposer, folderName, hostOf, isExplicitUrl, isSystemTab, sessionTab, pageTab, historyTab, settingsTab, toAddress, restoreTab } from "../src/lib/tabs.ts";

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

test("system tabs split from working tabs", () => {
  assert.equal(isSystemTab(sessionTab()), false);
  assert.equal(isSystemTab(pageTab("https://example.com")), false);
  assert.equal(isSystemTab(historyTab()), true);
  assert.equal(isSystemTab(settingsTab()), true);
});

test("a composer always has quote and attachment lists", () => {
  const fresh = ensureComposer();
  assert.deepEqual(fresh.quotes, []);
  assert.deepEqual(fresh.attachments, []);
  const saved = { input: "hi", mode: "work" as unknown as "build", model: "", effort: "medium", attachments: ["a.png"] };
  const partial = ensureComposer(saved);
  assert.equal(partial, saved);
  assert.equal(partial.input, "hi");
  assert.equal(partial.mode, "build");
  assert.deepEqual(partial.attachments, ["a.png"]);
  assert.deepEqual(partial.quotes, []);
  const kept = [{ project: "parzi", id: "prs", title: "PR badges" }];
  const quoted = ensureComposer({ quotes: kept, attachments: [] });
  assert.equal(quoted.quotes, kept);
  const ready = ensureComposer({ input: "", mode: "build", model: "", effort: "medium", attachments: [], quotes: [] });
  assert.equal(ensureComposer(ready), ready);
  assert.deepEqual(sessionTab().composer.quotes, []);
  assert.deepEqual(sessionTab().composer.attachments, []);
});

test("saved Work, Tasks, Brain and Projects tabs don't come back", () => {
  assert.equal(restoreTab({ id: "t1", kind: "tasks", title: "Tasks" }), null);
  assert.equal(restoreTab({ id: "t2", kind: "work", title: "Work" }), null);
  assert.equal(restoreTab({ id: "t5", kind: "brain", title: "Brain" }), null);
  assert.equal(restoreTab({ id: "t6", kind: "projects", title: "Projects" }), null);
  assert.equal(restoreTab({ id: "t3", kind: "settings", title: "Settings" }), null);
  assert.equal(restoreTab({ id: "t4", kind: "mystery", title: "?" }), null);
});
