import { test } from "node:test";
import assert from "node:assert/strict";
import { picturesIn } from "../src/lib/pictures.ts";

test("a diagram connector's drawing shows where it was made", () => {
  const out = JSON.stringify({ ok: true, svg: "/home/me/.parzi/scratch/s1/flow.svg", html: "/home/me/.parzi/scratch/s1/flow.html", items: 3 });
  assert.deepEqual(picturesIn([out]), ["/home/me/.parzi/scratch/s1/flow.svg"]);
});

test("pictures named in plain text are found, pages are not", () => {
  assert.deepEqual(picturesIn(["Saved C:\\shots\\a.png and /tmp/b.svg, see /tmp/c.html"]), ["C:\\shots\\a.png", "/tmp/b.svg"]);
});

test("a saved image path wins and repeats show once", () => {
  assert.deepEqual(picturesIn([JSON.stringify({ path: "/tmp/x.webp" }), "/tmp/x.webp"]), ["/tmp/x.webp"]);
});
