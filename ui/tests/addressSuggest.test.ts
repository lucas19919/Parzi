import { test } from "node:test";
import assert from "node:assert/strict";
import { get } from "svelte/store";
import { addressSuggest, type AddressSources } from "../src/lib/addressSuggest.ts";

const sources: AddressSources = {
  visits: [{ url: "https://github.com/lucas19919/Parzi", title: "Parzi", at: Date.now(), count: 4 }],
  marks: [{ url: "https://docs.rs/", title: "Docs.rs" }],
};

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

function make(phrases: (q: string) => Promise<string[]> = async () => [], live = () => true) {
  return addressSuggest({ sources: () => sources, phrases, live, delay: 0 });
}

test("typing at the end completes a known address and selects the added part", () => {
  const s = make();
  assert.deepEqual(s.type("git", true), { text: "github.com", select: [3, 10] });
  const rows = get(s).rows;
  assert.equal(rows[0].url, "https://github.com");
  assert.equal(rows.some((r) => r.kind === "history"), true);
  assert.deepEqual(s.type("git", false), { text: "git", select: null }, "deleting never completes");
});

test("search phrases arrive after a pause and only for what is still typed", async () => {
  const asked: string[] = [];
  const s = make(async (q) => {
    asked.push(q);
    return [`${q} tutorial`, `${q} docs`];
  });
  s.type("svelte stores", true);
  s.type("svelte store", true);
  await wait(10);
  assert.deepEqual(asked, ["svelte store"], "the earlier wait was dropped");
  assert.deepEqual(
    get(s).rows.map((r) => r.text),
    ["svelte store", "svelte store tutorial", "svelte store docs"],
  );
  s.type("example.com", true);
  await wait(10);
  assert.equal(asked.length, 1, "an address needs no phrases");
});

test("late phrases are dropped when the box no longer wants them", async () => {
  let open = true;
  const s = make(async (q) => [`${q} later`], () => open);
  s.type("rust", true);
  open = false;
  await wait(10);
  assert.equal(get(s).rows.some((r) => r.kind === "phrase"), false);
});

test("arrows wrap through the rows and back to what was typed; Escape closes first", () => {
  const s = make();
  s.type("docs", true);
  const n = get(s).rows.length;
  assert.ok(n >= 2, `rows: ${n}`);
  assert.equal(s.move(1), "docs.rs", "the completion row");
  assert.equal(s.chosen()?.url, "https://docs.rs");
  for (let i = 1; i < n; i++) s.move(1);
  assert.equal(s.move(1), "docs", "past the last row is what was typed");
  assert.equal(s.chosen(), null);
  assert.notEqual(s.move(-1), null, "up from the box goes to the last row");
  assert.equal(s.escape(), "docs");
  assert.deepEqual(get(s).rows, []);
  assert.equal(s.escape(), null, "nothing left to close");
  assert.equal(s.move(1), null);
});

test("hover points at a row and reset forgets everything", () => {
  const s = make();
  s.type("git", true);
  s.hover(0);
  assert.equal(s.chosen()?.text, "github.com");
  s.hover(99);
  assert.equal(get(s).active, 0, "out of range is ignored");
  s.reset();
  assert.deepEqual(get(s), { rows: [], active: -1, typed: "" });
});
