// Run: node --test ui/tests/roster.test.ts   (node >= 23 strips the types)
// The New project wizard proposes three models; the proposal is a rule over
// the catalog, not a hardcoded list, so it must hold for any catalog.
import test from "node:test";
import assert from "node:assert/strict";
import type { ModelRow } from "../src/lib/api.ts";
import { candidates, defaultRoster } from "../src/lib/workspace/roster.ts";

function model(id: string, price: number, over: Record<string, unknown> = {}) {
  return {
    id,
    name: id,
    context_limit: 200000,
    output_limit: 8192,
    price_in: price,
    price_out: price * 2,
    tools: true,
    vision: false,
    legacy: false,
    is_default: false,
    family: id,
    family_name: id,
    variant: null,
    ...over,
  };
}

function row(provider: string, models: ReturnType<typeof model>[], auth = "ok"): ModelRow {
  return {
    provider,
    auth,
    billing: "api_key",
    hint: "",
    models,
  } as unknown as ModelRow;
}

const CATALOG: ModelRow[] = [
  row("google", [model("gemini-2.5-flash", 0.3), model("gemini-1.0-pro", 1, { legacy: true })]),
  row("anthropic", [model("claude-fable-5-1", 6), model("claude-sonnet-4", 3)]),
  row("opencode", [model("muse-spark-1.3", 1.5), model("embed-only", 0.1, { tools: false })]),
  row("openai", [model("gpt-secret", 99)], "missing"),
];

test("only reachable, tool-capable, non-legacy models are candidates", () => {
  const specs = candidates(CATALOG).map((c) => c.spec).sort();
  assert.deepEqual(specs, [
    "anthropic/claude-fable-5-1",
    "anthropic/claude-sonnet-4",
    "google/gemini-2.5-flash",
    "opencode/muse-spark-1.3",
  ]);
});

test("header is cheapest, orchestrator is strongest, coder is a coding family", () => {
  const r = defaultRoster(CATALOG);
  assert.equal(r.header, "google/gemini-2.5-flash");
  assert.equal(r.orchestrator, "anthropic/claude-fable-5-1");
  // fable and sonnet both match a coder family; the dearer one wins.
  assert.equal(r.coder, "anthropic/claude-fable-5-1");
});

test("the coder falls back to the strongest when no coding family is signed in", () => {
  const plain = [row("google", [model("gemini-2.5-flash", 0.3), model("gemini-2.5-pro", 4)])];
  const r = defaultRoster(plain);
  assert.equal(r.header, "google/gemini-2.5-flash");
  assert.equal(r.orchestrator, "google/gemini-2.5-pro");
  assert.equal(r.coder, "google/gemini-2.5-pro");
});

test("no signed-in provider leaves every role on Auto", () => {
  assert.deepEqual(defaultRoster([]), { header: "", orchestrator: "", coder: "" });
  assert.deepEqual(defaultRoster([row("openai", [model("gpt-5", 2)], "expired")]), {
    header: "",
    orchestrator: "",
    coder: "",
  });
});
