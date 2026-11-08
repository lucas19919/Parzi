import { test } from "node:test";
import assert from "node:assert/strict";
import { cellLuminance, coverRect, inkFor, luminance, parsePosition, toCells } from "../src/lib/artInk.ts";

test("light ink goes on dark parts of the picture, dark ink on light parts", () => {
  assert.equal(inkFor(luminance(29, 59, 107)), "dark", "a navy sky");
  assert.equal(inkFor(luminance(60, 60, 60)), "dark");
  assert.equal(inkFor(luminance(160, 180, 210)), "light", "a pale blue sky");
  assert.equal(inkFor(luminance(217, 130, 91)), "light", "a sunset orange");
});

test("a picture covers its box the way object-fit: cover places it", () => {
  // A wide picture in a square box: cropped at the sides, centred.
  assert.deepEqual(coverRect(200, 100, 100, 100), { dx: -50, dy: 0, dw: 200, dh: 100 });
  // A tall picture, object-position y at the top: nothing cut above.
  assert.deepEqual(coverRect(100, 400, 100, 100, 0.5, 0), { dx: 0, dy: 0, dw: 100, dh: 400 });
  assert.deepEqual(coverRect(100, 400, 100, 100, 0.5, 1), { dx: 0, dy: -300, dw: 100, dh: 400 });
});

test("object-position reads percentages and keywords", () => {
  assert.deepEqual(parsePosition("50% 30%"), [0.5, 0.3]);
  assert.deepEqual(parsePosition("center top"), [0.5, 0]);
  assert.deepEqual(parsePosition(""), [0.5, 0.5]);
});

test("the brightness behind a box is read from the cells under it only", () => {
  // Two cells wide, one high, 10px each: black on the left, white on the right.
  const cells = toCells([0, 0, 0, 255, 255, 255, 255, 255], 2, 1, 10);
  const art = { left: 0, top: 0, width: 20, height: 10 };
  assert.equal(cellLuminance(cells, { left: 1, top: 1, width: 5, height: 5 }, art), 0);
  assert.equal(cellLuminance(cells, { left: 12, top: 1, width: 5, height: 5 }, art), 1);
  assert.equal(cellLuminance(cells, { left: 5, top: 0, width: 10, height: 10 }, art), 0.5);
  assert.equal(cellLuminance(cells, { left: 40, top: 0, width: 5, height: 5 }, art), null, "off the picture");
  assert.equal(cellLuminance(cells, { left: 0, top: 0, width: 0, height: 0 }, art), null, "hidden");
});
