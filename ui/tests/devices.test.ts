import { test } from "node:test";
import assert from "node:assert/strict";
import { DUO_CLOSED, cornerOverhang, MARGIN, PHONES, frameSize, lightInk, phoneFit, phoneFor, screenAt, viewport } from "../src/lib/devices.ts";

test("the phones are the iPhone 16 Pro, the Pixel 9 and the iPhone Duo", () => {
  assert.deepEqual([PHONES.iphone.width, PHONES.iphone.height, PHONES.iphone.scale], [402, 874, 3]);
  assert.deepEqual([PHONES.android.width, PHONES.android.height, PHONES.android.scale], [412, 923, 2.625]);
  assert.deepEqual([PHONES.duo.width, PHONES.duo.height, PHONES.duo.scale], [626, 890, 3]);
  assert.deepEqual([DUO_CLOSED.width, DUO_CLOSED.height, DUO_CLOSED.scale], [466, 678, 3]);
  assert.equal(PHONES.duo.tip, "As on an iPhone Duo");
});

test("the page gets the screen between the status bar and the home indicator", () => {
  assert.deepEqual([PHONES.iphone.top, PHONES.iphone.bottom], [62, 40]);
  assert.deepEqual([PHONES.android.top, PHONES.android.bottom], [40, 28]);
  assert.deepEqual(viewport(PHONES.iphone), { width: 402, height: 772 });
  assert.deepEqual(viewport(PHONES.android), { width: 412, height: 855 });
  assert.deepEqual(viewport(PHONES.duo), { width: 626, height: 810 });
  assert.deepEqual(viewport(DUO_CLOSED), { width: 466, height: 578 });
  for (const p of [...Object.values(PHONES), DUO_CLOSED]) {
    const v = viewport(p);
    assert.ok(Number.isInteger(v.width) && Number.isInteger(v.height), "the desktop takes whole pixels");
    // The bars hold the camera: the island or punch hole fits above the page.
    assert.ok(p.top >= 40 && p.bottom >= 20, p.label);
    // The page is a sharp rectangle: its corners stay inside the round ones.
    assert.ok(cornerOverhang(p) < 1, `${p.label} overhangs ${cornerOverhang(p)}px`);
  }
  assert.equal(cornerOverhang({ ...PHONES.iphone, bottom: 60 }), 0);
  assert.ok(cornerOverhang({ ...PHONES.iphone, radius: 55, bottom: 34 }) > 4, "a shallow bar would show the page's corner");
});

test("the Duo opens and closes, still Safari on an iPhone", () => {
  assert.equal(phoneFor("duo"), PHONES.duo);
  assert.equal(phoneFor("duo", true), DUO_CLOSED);
  assert.equal(phoneFor("iphone", true), PHONES.iphone, "only the Duo folds");
  assert.equal(DUO_CLOSED.userAgent, PHONES.iphone.userAgent);
  assert.equal(PHONES.duo.userAgent, PHONES.iphone.userAgent);
  assert.notDeepEqual(viewport(PHONES.duo), viewport(DUO_CLOSED));
});

test("the drawing is the screen with the body and buttons around it", () => {
  const p = PHONES.iphone;
  const size = frameSize(p);
  assert.equal(size.width - p.width, 2 * screenAt(p));
  assert.equal(size.height - p.height, 2 * screenAt(p));
  assert.ok(screenAt(p) > p.bezel + p.edge, "buttons stand out of the body");
});

test("a phone is drawn at full size when it fits, smaller when it doesn't", () => {
  for (const p of [...Object.values(PHONES), DUO_CLOSED]) {
    const size = frameSize(p);
    const roomy = { width: size.width + 2 * MARGIN, height: size.height + 2 * MARGIN };
    assert.equal(phoneFit(p, roomy), 1, p.label);
    assert.equal(phoneFit(p, { width: 4000, height: 4000 }), 1, "never above full size");
    for (const room of [
      { width: 1200, height: 600 },
      { width: 400, height: 1400 },
      { width: 900, height: 700 },
    ]) {
      const fit = phoneFit(p, room);
      assert.ok(fit < 1, `${p.label} is scaled down in ${room.width}x${room.height}`);
      assert.ok(size.width * fit + 2 * MARGIN <= room.width, `${p.label} fits the width: ${fit}`);
      assert.ok(size.height * fit + 2 * MARGIN <= room.height, `${p.label} fits the height: ${fit}`);
      assert.equal(fit, Math.round(fit * 100) / 100, "rounded to hundredths");
      // The page rectangle is the viewport times the fit, inside the drawing.
      const v = viewport(p);
      assert.ok(screenAt(p) + p.top + v.height + p.bottom + screenAt(p) === size.height);
      assert.ok(v.width * fit < size.width * fit);
    }
  }
  assert.ok(phoneFit(PHONES.iphone, { width: 0, height: 0 }) > 0, "never zero");
});

test("open, the Duo is the bigger screen", () => {
  assert.ok(PHONES.duo.width > DUO_CLOSED.width && PHONES.duo.height > DUO_CLOSED.height);
  const open = phoneFit(PHONES.duo, { width: 900, height: 900 });
  const shut = phoneFit(DUO_CLOSED, { width: 900, height: 900 });
  assert.ok(open < shut, "open needs more room");
});

test("the status bar is white on dark pages", () => {
  assert.equal(lightInk("rgb(255, 255, 255)"), false);
  assert.equal(lightInk("rgb(14, 20, 32)"), true);
  assert.equal(lightInk("rgba(0, 0, 0, 1)"), true);
  assert.equal(lightInk("#0e1420"), true);
  assert.equal(lightInk("#fff"), false);
  assert.equal(lightInk("rgb(37, 99, 235)"), true, "a strong blue takes white");
  assert.equal(lightInk("rgb(250, 204, 21)"), false, "yellow takes black");
  assert.equal(lightInk(undefined), false);
  assert.equal(lightInk("transparent"), false);
});

test("each phone says what it is", () => {
  assert.match(PHONES.iphone.userAgent, /iPhone/);
  assert.match(PHONES.android.userAgent, /Android .*Mobile/);
});
