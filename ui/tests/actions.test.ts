import { test } from "node:test";
import assert from "node:assert/strict";
import { availableActions, defineActions, describeActions, runAction } from "../src/lib/actions.ts";

test("actions are listed with their arguments and only when they can run", () => {
  let running = false;
  const off = defineActions([
    { id: "tabs.new", title: "Open a new tab", run: () => "opened" },
    { id: "session.stop", title: "Stop the run", when: () => running, run: () => "stopped" },
    { id: "composer.mode", title: "Switch mode", args: [{ name: "mode", type: "string", required: true, choices: ["agent", "web"] }], run: () => {} },
  ]);
  assert.deepEqual(availableActions().map((a) => a.id), ["composer.mode", "tabs.new"]);
  running = true;
  assert.ok(availableActions().some((a) => a.id === "session.stop"));
  assert.equal(describeActions("mode"), "composer.mode: Switch mode { mode: agent|web }");
  assert.match(describeActions("nothing like it"), /No action matches/);
  off();
  assert.deepEqual(availableActions(), []);
});

test("arguments are checked, coerced and named back when wrong", async () => {
  let got: Record<string, unknown> = {};
  const off = defineActions([
    {
      id: "window.zoom",
      title: "Zoom",
      args: [
        { name: "level", type: "number", required: true },
        { name: "on", type: "boolean" },
        { name: "mode", type: "string", choices: ["fit", "fill"] },
      ],
      run: (a) => {
        got = a;
        return "zoomed";
      },
    },
  ]);
  assert.deepEqual(await runAction("window.zoom", { level: "2", on: "yes", mode: "FILL" }), { text: "zoomed" });
  assert.deepEqual(got, { level: 2, on: true, mode: "fill" });
  await assert.rejects(runAction("window.zoom", {}), /needs level: number/);
  await assert.rejects(runAction("window.zoom", { level: 1, mode: "huge" }), /mode is one of fit, fill, not "huge"/);
  await assert.rejects(runAction("window.zom"), /No action "window.zom". Try window.zoom/);
  off();
});

test("a dangerous action waits for the person's yes", async () => {
  let ran = 0;
  const off = defineActions([{ id: "project.delete", title: "Delete the project", danger: true, run: () => void ran++ }]);
  assert.deepEqual(await runAction("project.delete"), { needs_yes: "Delete the project" });
  assert.equal(ran, 0);
  assert.deepEqual(await runAction("project.delete", {}, true), { text: "Done: Delete the project." });
  assert.equal(ran, 1);
  assert.match(describeActions("project"), /\(asks the person\)/);
  off();
});
