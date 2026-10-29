import { test } from "node:test";
import assert from "node:assert/strict";
import { readdir, readFile } from "node:fs/promises";

async function svelteFiles(dir: URL): Promise<URL[]> {
  const out: URL[] = [];
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const url = new URL(entry.name + (entry.isDirectory() ? "/" : ""), dir);
    if (entry.isDirectory()) out.push(...(await svelteFiles(url)));
    else if (entry.name.endsWith(".svelte")) out.push(url);
  }
  return out;
}

test("no writes through bind:this element variables", async () => {
  const bad: string[] = [];
  for (const file of await svelteFiles(new URL("../src/", import.meta.url))) {
    const src = await readFile(file, "utf8");
    const script = (/<script[^>]*>([\s\S]*?)<\/script>/.exec(src)?.[1] ?? "").replace(/\/\*[\s\S]*?\*\/|\/\/.*$/gm, "");
    const bound = new Set([...src.matchAll(/bind:this=\{(\w+)\}/g)].map((m) => m[1]));
    for (const name of bound) {
      const write = new RegExp(`\\b${name}\\??\\.[\\w.]+\\s*(=|\\+=|-=)(?!=)`, "g");
      for (const m of script.matchAll(write)) bad.push(`${file.pathname.split("/src/")[1]}: ${m[0].trim()}`);
    }
  }
  assert.deepEqual(bad, []);
});
