import test from "node:test";
import assert from "node:assert/strict";
import { isChatThread } from "../src/lib/nav.ts";

test("Inbox is always a chat", () => {
  assert.equal(isChatThread("default", ["acme"]), true);
  assert.equal(isChatThread("", ["acme"]), true);
  assert.equal(isChatThread(undefined, ["acme"]), true);
});

test("a hub workspace session is not a chat", () => {
  assert.equal(isChatThread("acme", ["acme"]), false);
});

test("a leftover named folder stays a chat if it is not a hub workspace", () => {
  assert.equal(isChatThread("old-notes", ["acme"]), true);
});
