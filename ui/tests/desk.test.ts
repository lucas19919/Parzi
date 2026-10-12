import { test } from "node:test";
import assert from "node:assert/strict";
import { deskRows } from "../src/lib/desk.ts";

const composer = { input: "", mode: "build", model: "", effort: "medium", attachments: [] };

test("a page tab syncs the agent that owns it, a session tab its session", () => {
  const rows = deskRows([
    { id: "p1", kind: "page", title: "Docs", url: "https://docs.rs", owner: "s1", sessionId: null, composer },
    { id: "p2", kind: "page", title: "Bank", url: "https://bank.example", composer },
    { id: "t1", kind: "session", title: "Fix", sessionId: "s2", cwd: "C:/src", composer },
    { id: "d1", kind: "session", title: "New session", sessionId: null, cwd: "D:/draft", composer },
    { id: "x1", kind: "settings", title: "Settings", composer },
  ] as never);
  assert.deepEqual(
    rows.map((r) => [r.id, r.kind, r.session_id, r.cwd]),
    [
      ["p1", "browser", "s1", ""],
      ["p2", "browser", "", ""],
      ["t1", "session", "s2", "C:/src"],
      ["d1", "session", "", "D:/draft"],
      ["x1", "settings", "", ""],
    ],
  );
  assert.equal(rows[0].url, "https://docs.rs");
});
