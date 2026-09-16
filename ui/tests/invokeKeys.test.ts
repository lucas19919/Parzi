/**
 * Tauri v2 maps a command's snake_case Rust arguments to camelCase keys.
 * A snake_case key in `invoke(...)` is silently dropped: an `Option` arg
 * reads as None, a required one fails. `session_id` did this to every
 * follow-up chat message (each started a new thread) and `base64_data`
 * to wallpaper upload. Guard every top-level key in api.ts.
 */
import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";

/** Top-level keys of the object literal starting right after `{` at `from`. */
function topKeys(src: string, from: number): string[] {
  const keys: string[] = [];
  let depth = 1;
  let token = "";
  let inValue = false;
  for (let i = from; i < src.length && depth > 0; i++) {
    const c = src[i];
    if (c === "{" || c === "[" || c === "(") depth++;
    else if (c === "}" || c === "]" || c === ")") depth--;
    // Nested values are serde structs and keep their snake_case fields.
    if (depth === 0 || (depth === 1 && c === ",")) {
      if (!inValue && token.trim()) keys.push(token.trim()); // shorthand `{ repo }`
      token = "";
      inValue = false;
    } else if (depth === 1 && c === ":" && !inValue) {
      keys.push(token.trim());
      inValue = true;
    } else if (depth === 1 && !inValue) {
      token += c;
    }
  }
  return keys;
}

test("invoke argument keys are camelCase", async () => {
  const src = await readFile(new URL("../src/lib/api.ts", import.meta.url), "utf8");
  const call = /invoke<[^(]*>\(\s*"([a-z0-9_]+)"\s*,\s*\{/g;
  const bad: string[] = [];
  let seen = 0;
  for (let m; (m = call.exec(src)); ) {
    seen++;
    for (const k of topKeys(src, call.lastIndex)) {
      if (/^[a-z0-9]+_[a-z0-9_]+$/.test(k)) bad.push(`${m[1]}: ${k}`);
    }
  }
  assert.ok(seen > 20, `expected to scan the command calls, saw ${seen}`);
  assert.deepEqual(bad, []);
});
