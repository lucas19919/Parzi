import { test } from "node:test";
import assert from "node:assert/strict";
import { folderName, hostOf, isExplicitUrl, isSystemTab, sessionTab, pageTab, historyTab, brainTab, settingsTab, toAddress } from "../src/lib/tabs.ts";
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

test("system tabs split from working tabs", () => {
  assert.equal(isSystemTab(sessionTab()), false);
  assert.equal(isSystemTab(pageTab("https://example.com")), false);
  assert.equal(isSystemTab(historyTab()), true);
  assert.equal(isSystemTab(brainTab()), true);
  assert.equal(isSystemTab(settingsTab()), true);
});
