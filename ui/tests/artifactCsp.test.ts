import { test } from "node:test";
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { readFile } from "node:fs/promises";

// srcdoc frames inherit the app's CSP, so the artifact height script runs
// only while its hash is listed in tauri.conf.json's script-src.
test("the artifact height script is allowed by the app CSP", async () => {
  const card = await readFile(new URL("../src/lib/widgets/ArtifactCard.svelte", import.meta.url), "utf8");
  const open = "const RESIZE = `<script>";
  const start = card.indexOf(open) + open.length;
  const end = card.indexOf("/script>`;", start) - 2;
  assert.ok(start > open.length && end > start, "RESIZE script not found");
  const hash = createHash("sha256").update(card.slice(start, end), "utf8").digest("base64");
  const conf = JSON.parse(await readFile(new URL("../../src-tauri/tauri.conf.json", import.meta.url), "utf8"));
  const scriptSrc = String(conf.app.security.csp)
    .split(";")
    .map((d) => d.trim())
    .find((d) => d.startsWith("script-src "));
  assert.ok(scriptSrc?.split(/\s+/).includes(`'sha256-${hash}'`), `add 'sha256-${hash}' to script-src`);
});
