import { test } from "node:test";
import assert from "node:assert/strict";
import { permissionBlocked, permissionFor } from "../src/lib/permission.ts";

test("every session starts on full unless the config locks it down", () => {
  for (const m of ["ask", "auto", "full", "supervised", "", "FULL", "turbo", null, undefined]) {
    assert.equal(permissionFor(m), "full");
  }
  assert.equal(permissionFor("deny"), "deny");
  assert.equal(permissionFor(" deny "), "deny");
});

test("every composer choice is open unless Settings locks agents down", () => {
  for (const c of ["deny", "supervised", "edits", "auto", "full"]) assert.equal(permissionBlocked(c, "auto"), "");
  for (const c of ["deny", "supervised", "edits", "auto", "full"]) assert.equal(permissionBlocked(c, "ask"), "");
  for (const c of ["deny", "supervised", "edits", "auto", "full"]) assert.equal(permissionBlocked(c, "full"), "");
  assert.equal(permissionBlocked("deny", "deny"), "");
  for (const c of ["supervised", "edits", "auto", "full"]) assert.notEqual(permissionBlocked(c, "deny"), "");
});
