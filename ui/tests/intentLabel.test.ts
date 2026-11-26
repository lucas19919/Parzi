import { test } from "node:test";
import assert from "node:assert/strict";
import { commandIntent, isShellStepLabel, stepIntent } from "../src/lib/intentLabel.ts";

test("maps common git, build, test, lint, and server commands to short intent names", () => {
  assert.equal(commandIntent('git commit -m "fix stuff"', undefined, true), "Committing…");
  assert.equal(commandIntent('git commit -m "fix stuff"', undefined, false), "Committed");
  assert.equal(commandIntent("git push origin main", undefined, true), "Pushing…");
  assert.equal(commandIntent("cargo test --manifest-path src-tauri/Cargo.toml", undefined, true), "Running tests…");
  assert.equal(commandIntent("npm --prefix ui run check", undefined, true), "Checking code…");
  assert.equal(commandIntent("npm --prefix ui run build", undefined, false), "Built");
  assert.equal(commandIntent("npm run dev", ["http://localhost:5173/"], true), "Running localhost:5173");
  assert.equal(commandIntent("npm run dev", undefined, true), "Running localhost…");
  assert.equal(commandIntent("rg foo ui/src", undefined, true), "Searching code…");
  assert.equal(commandIntent("cd ui && npm install", undefined, true), "Installing packages…");
});

test("maps tool step labels to clean file/action intents", () => {
  assert.equal(stepIntent("$ cargo clippy --all-targets", true), "Checking code…");
  assert.equal(stepIntent("Run git push origin main", false), "Pushed");
  assert.equal(stepIntent("Read D:\\Repos\\Parzi\\ui\\src\\App.svelte", true), "Reading App.svelte…");
  assert.equal(stepIntent("Edit ui/src/theme.css", false), "Edited theme.css");
  assert.equal(stepIntent("Search handleLinkClick", true), "Searching code…");
  assert.ok(isShellStepLabel("$ cargo test"));
  assert.ok(isShellStepLabel("Run npm test"));
  assert.ok(!isShellStepLabel("Read App.svelte"));
});
