import { test } from "node:test";
import assert from "node:assert/strict";
import { relativeTo, sameDir, shortAttachment } from "../src/lib/paths.ts";

test("sameDir is slash- and case-insensitive", () => {
  assert.equal(sameDir("C:\\Proj\\App", "c:/proj/app/"), true);
  assert.equal(sameDir("C:\\a", "C:\\b"), false);
  assert.equal(sameDir("", "C:\\b"), false);
});

test("relativeTo strips the root prefix", () => {
  assert.equal(relativeTo("C:\\proj\\src\\a.ts", "C:\\proj"), "src\\a.ts");
  assert.equal(relativeTo("/home/u/a.ts", "/home/u"), "a.ts");
  assert.equal(relativeTo("/other/a.ts", "/home/u"), "/other/a.ts");
});

test("shortAttachment truncates long basenames", () => {
  assert.equal(shortAttachment("a/b/c.ts"), "c.ts");
  assert.ok(shortAttachment(`x/${"y".repeat(30)}.ts`).endsWith("…"));
});
