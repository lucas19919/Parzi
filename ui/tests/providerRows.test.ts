import { test } from "node:test";
import assert from "node:assert/strict";
import { chipOf, effortsFor, familyOf, modelLabel, prettyModel } from "../src/lib/providerRows.ts";

const status = (provider: string, models: { id: string; efforts: string[]; is_default?: boolean }[]) =>
  ({ provider, state: "ready", models }) as never;

test("Claude keeps an effort picker while its model list is missing", () => {
  assert.deepEqual(effortsFor("claude/sonnet", [status("claude", [])]), ["low", "medium", "high", "xhigh", "max"]);
  assert.deepEqual(effortsFor("codex/gpt-5", []), ["low", "medium", "high", "xhigh", "max"]);
});

test("a listed model decides its own efforts", () => {
  const board = [
    status("claude", [
      { id: "sonnet", efforts: ["low", "high"], is_default: true },
      { id: "haiku", efforts: [] },
    ]),
  ];
  assert.deepEqual(effortsFor("claude/sonnet", board), ["low", "high"]);
  assert.deepEqual(effortsFor("claude/haiku", board), [], "a model with no efforts really has none");
});

test("model ids read as names", () => {
  assert.equal(prettyModel("opencode/muse-spark-1.3-contributor-free"), "Muse Spark 1.3 Contributor Free");
  assert.equal(prettyModel("anthropic/claude-sonnet-5-5"), "Claude Sonnet 5.5");
  assert.equal(prettyModel("grok-4.6"), "Grok 4.6");
  assert.equal(prettyModel("openai/gpt-5"), "GPT 5");
  assert.equal(prettyModel("claude-opus-4-1-20250805"), "Claude Opus 4.1 20250805");
});

test("the omnibar chip keeps the model and its version", () => {
  assert.deepEqual(chipOf("OpenCode Go/Muse Spark 1.3 Contributor"), { title: "Muse", version: "1.3" });
  assert.deepEqual(chipOf("OpenCode/Muse Wonder"), { title: "Muse Wonder", version: "" });
  assert.deepEqual(chipOf("OpenCode/Claude Sonnet 5.5"), { title: "Sonnet", version: "5.5" });
  assert.deepEqual(chipOf("OpenCode/Claude Haiku 4.5 (latest)"), { title: "Haiku", version: "4.5" });
  assert.deepEqual(chipOf("OpenCode/Big Pickle"), { title: "Big Pickle", version: "" });
  assert.deepEqual(chipOf("OpenCode/Beam Free"), { title: "Beam", version: "" });
  assert.deepEqual(chipOf("OpenCode/GPT-5.4 Mini"), { title: "GPT-5.4 Mini", version: "" });
  assert.deepEqual(chipOf("OpenCode/GPT-5.3 Codex Spark"), { title: "GPT-5.3 Codex", version: "" });
  assert.deepEqual(chipOf("OpenCode/GLM-5.3-Flash"), { title: "GLM-5.3", version: "" });
  assert.deepEqual(chipOf("OpenCode Go/Kimi K2.7 Code"), { title: "Kimi", version: "K2.7" });
  assert.deepEqual(chipOf("Grok 4.7"), { title: "Grok", version: "4.7" });
});

test("an agent's own model name wins", () => {
  assert.equal(modelLabel({ id: "gemini-pro-agent", name: "Gemini 3.1 Pro (High)" }), "Gemini 3.1 Pro (High)");
  assert.equal(modelLabel({ id: "opencode/big-pickle", name: "opencode/big-pickle" }), "Big Pickle");
});

test("a saved effort variant becomes its family at that effort", () => {
  const board = [
    {
      provider: "antigravity",
      models: [
        { id: "gemini-4-neon", name: "Gemini 4 Neon", is_default: true, efforts: ["low", "medium", "high", "xhigh"], variants: true },
        { id: "solo-high", name: "Solo (High)", is_default: false, efforts: ["low", "high"] },
      ],
    },
  ] as unknown as Parameters<typeof familyOf>[1];
  assert.deepEqual(familyOf("antigravity/gemini-4-neon-high", board), { model: "antigravity/gemini-4-neon", effort: "high" });
  assert.equal(familyOf("antigravity/gemini-4-neon", board), null, "already a family");
  assert.equal(familyOf("antigravity/gemini-4-neon-max", board), null, "an effort the family lacks");
  assert.equal(familyOf("antigravity/solo-high", board), null, "listed as it is");
});
