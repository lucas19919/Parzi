import { test } from "node:test";
import assert from "node:assert/strict";
import { laneIcon, normLane, permissionBlocked, permissionFor } from "../src/lib/lanes.ts";

test("normLane maps legacy names", () => {
  assert.equal(normLane("code"), "build");
  assert.equal(normLane("research"), "work");
  assert.equal(normLane("build"), "build");
  assert.equal(normLane("work"), "work");
  assert.equal(normLane(""), "");
});

test("laneIcon maps to brain/bot/chat", () => {
  assert.equal(laneIcon("work"), "brain");
  assert.equal(laneIcon("research"), "brain");
  assert.equal(laneIcon("build"), "bot");
  assert.equal(laneIcon("code"), "bot");
  assert.equal(laneIcon(""), "chat");
});

test("permissionFor follows Settings and never defaults to full access", () => {
  assert.equal(permissionFor("ask"), "supervised");
  assert.equal(permissionFor("auto"), "auto");
  assert.equal(permissionFor("deny"), "deny");
  assert.equal(permissionFor(" auto "), "auto");
  for (const odd of ["", "full", "FULL", "turbo", null, undefined]) assert.equal(permissionFor(odd), "supervised");
});

test("every composer choice is open unless Settings locks agents down", () => {
  for (const c of ["deny", "supervised", "edits", "auto", "full"]) assert.equal(permissionBlocked(c, "auto"), "");
  for (const c of ["deny", "supervised", "edits", "auto", "full"]) assert.equal(permissionBlocked(c, "ask"), "");
  assert.equal(permissionBlocked("deny", "deny"), "");
  for (const c of ["supervised", "edits", "auto", "full"]) assert.notEqual(permissionBlocked(c, "deny"), "");
});
