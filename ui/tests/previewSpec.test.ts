import { test } from "node:test";
import assert from "node:assert/strict";
import { parsePreview, previewProblem, tokenStyle } from "../src/lib/previewSpec.ts";

test("a preview mounts the omnibar and keeps token overrides", () => {
  const live = parsePreview('{"component":"omnibar","variant":1,"input":"Ask","tokens":{"radius":"20px","panel":"#ffffff","nope":"x"}}');
  assert.ok(live);
  assert.equal(live.variant, 1);
  assert.equal(live.extra.input, "Ask");
  assert.equal("tokens" in live.extra, false);
  assert.match(live.style, /--radius:20px/);
  assert.match(live.style, /--panel:#ffffff/);
  assert.doesNotMatch(live.style, /nope/);
});

test("web is variant 2, and a dead variant is refused", () => {
  const web = parsePreview('{"component":"omnibar","variant":2}');
  assert.equal(web?.variant, 2);
  assert.equal(web?.style, "");
  assert.equal(parsePreview('{"component":"omnibar","variant":7}'), null);
  assert.match(previewProblem('{"component":"omnibar","variant":7}'), /Valid: 1, 2/);
});

test("token values stay colors and lengths", () => {
  const dirty = tokenStyle({ radius: "16px;color:red", bg: "#fff", space: "4px" });
  assert.match(dirty, /--bg:#fff/);
  assert.match(dirty, /--space:4px/);
  assert.doesNotMatch(dirty, /color:red/);
  assert.equal(tokenStyle({ radius: "16px;color:red" }), "");
  const ok = tokenStyle({ bg: "#111111", text: "#eeeeee" });
  assert.match(ok, /--bg:#111111/);
  assert.match(ok, /--muted:color-mix/);
  assert.match(ok, /--panel:color-mix/);
});
