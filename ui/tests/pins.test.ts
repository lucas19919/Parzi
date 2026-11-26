import { test } from "node:test";
import assert from "node:assert/strict";
import { sessionPins, targetsSummary } from "../src/lib/pins.ts";
import type { HostJob } from "../src/lib/api.ts";

const host = (id: string, urls: string[], running = true): HostJob => ({ id, cmd: "npm run dev", workdir: "", urls, running, exit: null, started: 0, ms: 0 });

test("a dev server pins its first localhost address, named host:port", () => {
  const pins = sessionPins([host("a", ["http://192.168.1.4:5173/", "http://localhost:5173/", "http://127.0.0.1:5173/"])]);
  assert.deepEqual(pins, [{ kind: "url", label: "localhost:5173", target: "http://localhost:5173/" }]);
});

test("stopped servers, servers with no local address and repeats are left out", () => {
  const pins = sessionPins([
    host("a", ["http://localhost:1420"]),
    host("b", ["http://localhost:8000"], false),
    host("c", ["https://example.com"]),
    host("d", ["http://localhost:1420"]),
    host("e", ["http://[::1]:3000"]),
  ]);
  assert.deepEqual(pins.map((p) => p.label), ["localhost:1420", "[::1]:3000"]);
});

test("the plan's document comes last, as Plan", () => {
  const pins = sessionPins([host("a", ["http://localhost:1420"])], "C:\s\plan.md");
  assert.deepEqual(pins.at(-1), { kind: "file", label: "Plan", target: "C:\s\plan.md" });
  assert.deepEqual(sessionPins([], "  "), []);
  assert.deepEqual(sessionPins([], null), []);
});

test("targets: the open ones in order, which are being worked on, and a done count", () => {
  const s = targetsSummary({
    goal: "Ship",
    targets: [
      { title: "One", state: "done" },
      { title: "Two", state: "working" },
      { title: " ", state: "todo" },
      { title: "Four", state: "done" },
    ],
  });
  assert.deepEqual(s, { open: [{ title: "Two", working: true }, { title: "Untitled", working: false }], done: 2 });
});

test("no plan, or a plan with no targets, shows no row", () => {
  assert.equal(targetsSummary(null), null);
  assert.equal(targetsSummary(undefined), null);
  assert.equal(targetsSummary({ goal: "Ship", targets: [] }), null);
  assert.deepEqual(targetsSummary({ goal: "", targets: [{ title: "A", state: "done" }] }), { open: [], done: 1 });
});


test("a session plan with steps pins as a live checklist even without a doc path", () => {
  const items = [
    { title: "One", state: "done" as const },
    { title: "Two", state: "active" as const },
    { title: "Three", state: "pending" as const },
  ];
  assert.deepEqual(sessionPins([], null, items), [
    { kind: "plan", label: "Plan · 1/3", target: "plan" },
  ]);
  assert.deepEqual(sessionPins([], "C:\\s\\plan.md", items), [
    { kind: "plan", label: "Plan · 1/3", target: "C:\\s\\plan.md" },
  ]);
});
