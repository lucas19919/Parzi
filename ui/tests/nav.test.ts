import test from "node:test";
import assert from "node:assert/strict";
import { isChatThread } from "../src/lib/nav.ts";

test("Inbox is always a chat", () => {
  assert.equal(isChatThread("default", ["acme"]), true);
  assert.equal(isChatThread("", ["acme"]), true);
  assert.equal(isChatThread(undefined, ["acme"]), true);
});

test("a project role session is not a chat", () => {
  assert.equal(isChatThread("checkout-flow", ["checkout-flow"]), false);
});

test("a chat in a workspace stays a chat", () => {
  assert.equal(isChatThread("acme", ["checkout-flow"]), true);
});
