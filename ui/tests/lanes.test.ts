import { test } from "node:test";
import assert from "node:assert/strict";
import { laneIcon, normLane, permissionBlocked, permissionFor, permissionGate } from "../src/lib/lanes.ts";

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

test("the composer only offers what the Settings floor lets through", () => {
  for (const c of ["deny", "supervised", "edits", "auto", "full"]) assert.equal(permissionBlocked(c, "auto"), "");
  // Auto under Ask behaves as Ask (the backend caps it), so it stays
  // selectable; only Lockdown disables.
  for (const c of ["deny", "supervised", "edits", "auto", "full"]) assert.equal(permissionBlocked(c, "ask"), "");
  assert.equal(permissionBlocked("deny", "deny"), "");
  for (const c of ["supervised", "edits", "auto", "full"]) assert.notEqual(permissionBlocked(c, "deny"), "");
});

test("full access says it lifts the Ask floor instead of looking free", () => {
  const lift = permissionGate("full", "ask");
  assert.equal(lift.off, "");
  assert.ok(lift.lift.length > 0);
  assert.equal(permissionGate("full", "auto").lift, "");
  assert.equal(permissionGate("auto", "auto").off, "");
  assert.ok(permissionGate("full", "deny").off.length > 0);
});
