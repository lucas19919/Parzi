import { test } from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

test("the app's policy lets HTML artifacts load from their own origin", async () => {
  const conf = JSON.parse(await readFile(new URL("../../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  const frameSrc = String(conf.app.security.csp)
    .split(";")
    .map((d) => d.trim())
    .find((d) => d.startsWith("frame-src "));
  assert.ok(frameSrc?.includes("parzi-art"), "add the parzi-art scheme to frame-src");
});

test("an artifact frame stays sandboxed with no way to the app", async () => {
  const card = await readFile(new URL("../src/lib/widgets/ArtifactCard.svelte", import.meta.url), "utf8");
  for (const m of card.matchAll(/sandbox="([^"]*)"/g)) assert.equal(m[1], "allow-scripts");
  assert.match(card, /sandbox="allow-scripts"/);
});
