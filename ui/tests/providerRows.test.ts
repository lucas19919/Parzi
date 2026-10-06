import { test } from "node:test";
import assert from "node:assert/strict";
import { modelLabel, prettyModel } from "../src/lib/providerRows.ts";

test("model ids read as names", () => {
  assert.equal(prettyModel("opencode/muse-spark-1.3-contributor-free"), "Muse Spark 1.3 Contributor Free");
  assert.equal(prettyModel("anthropic/claude-sonnet-5-5"), "Claude Sonnet 5.5");
  assert.equal(prettyModel("grok-4.6"), "Grok 4.6");
  assert.equal(prettyModel("openai/gpt-5"), "GPT 5");
  assert.equal(prettyModel("claude-opus-4-1-20250805"), "Claude Opus 4.1 20250805");
});

test("an agent's own model name wins", () => {
  assert.equal(modelLabel({ id: "gemini-pro-agent", name: "Gemini 3.1 Pro (High)" }), "Gemini 3.1 Pro (High)");
  assert.equal(modelLabel({ id: "opencode/big-pickle", name: "opencode/big-pickle" }), "Big Pickle");
});
