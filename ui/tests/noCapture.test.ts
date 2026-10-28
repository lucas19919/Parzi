import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

function src(name: string): string {
  return readFileSync(new URL(`../src/lib/${name}`, import.meta.url), "utf8");
}

test("clicks do not take pointer capture", () => {
  for (const name of ["TabBar.svelte", "BrainView.svelte", "gestures.ts"]) {
    assert.equal(src(name).includes("setPointerCapture"), false, name);
  }
});

test("the shell does not start a window drag from the page", () => {
  const chrome = src("windowChrome.ts");
  assert.equal(chrome.includes(".startDragging("), false);
  assert.ok(chrome.includes('invoke("begin_window_drag")'));
});

test("showing a page does not take focus from the shell", () => {
  const page = src("PageView.svelte");
  const api = src("api.ts");
  assert.ok(page.includes("browserShow(tab.id, rect, url, tab.owner ?? null, false)"));
  assert.ok(api.includes("focus = false"));
});
