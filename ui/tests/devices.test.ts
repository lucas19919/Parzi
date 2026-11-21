import { test } from "node:test";
import assert from "node:assert/strict";
import { BEZEL, PHONES, phoneFit } from "../src/lib/devices.ts";

test("a phone is drawn at full size when it fits, smaller when it doesn't", () => {
  const p = PHONES.iphone;
  assert.equal(phoneFit(p, { width: 1200, height: 1200 }), 1);
  const fit = phoneFit(p, { width: 1200, height: 600 });
  assert.ok(fit < 1 && p.height * fit + 2 * BEZEL <= 600, `fits the height: ${fit}`);
  assert.equal(fit, Math.floor(fit * 100) / 100, "rounded to hundredths");
  assert.ok(phoneFit(p, { width: 0, height: 0 }) > 0, "never zero");
});

test("each phone says what it is", () => {
  assert.match(PHONES.iphone.userAgent, /iPhone/);
  assert.match(PHONES.android.userAgent, /Android .*Mobile/);
});
