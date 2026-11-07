import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

function src(name: string): string {
  return readFileSync(new URL(`../src/lib/${name}`, import.meta.url), "utf8");
}

test("clicks do not take pointer capture", () => {
  for (const name of ["TabBar.svelte", "gestures.ts"]) {
    assert.equal(src(name).includes("setPointerCapture"), false, name);
  }
});

test("the shell does not start a window drag from the page", () => {
  const chrome = src("windowChrome.ts");
  assert.equal(chrome.includes(".startDragging("), false);
  assert.ok(chrome.includes('invoke("begin_window_drag")'));
});

test("showing a page does not take focus from the shell", () => {
  const call = src("api.ts").match(/invoke<void>\("browser_show", \{([^}]*)\}\)/);
  assert.ok(call, "api.ts calls browser_show");
  assert.equal(call[1].includes("focus"), false);
  const rust = readFileSync(new URL("../../src-tauri/src/browser.rs", import.meta.url), "utf8");
  const between = (from: string, to: string) => rust.slice(rust.indexOf(from), rust.indexOf(to, rust.indexOf(from)));
  const show = between("pub fn browser_show(", "\n}\n");
  const mount = between("fn mount_page(", "\n}\n");
  assert.ok(show.length > 0 && mount.length > 0, "browser.rs still has browser_show and mount_page");
  assert.equal(show.includes("focus"), false, "browser_show takes no focus switch");
  assert.equal(mount.includes("set_focus"), false, "mounting a page never focuses it");
});
