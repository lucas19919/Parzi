import { test } from "node:test";
import assert from "node:assert/strict";
import { phoneFor, phoneUrl, qrPath } from "../src/lib/qr.ts";

test("a phone gets the network address, never localhost", () => {
  assert.equal(phoneUrl(["http://localhost:5173", "http://192.168.1.10:5173"]), "http://192.168.1.10:5173");
  assert.equal(phoneUrl(["http://127.0.0.1:8000", "http://100.101.2.3:8000/app"]), "http://100.101.2.3:8000/app");
  assert.equal(phoneUrl(["http://localhost:5173", "http://app.localhost:3000", "http://[::1]:4000"]), null);
  assert.equal(phoneUrl([]), null);
});

test("a QR code is a square of dark runs", () => {
  const { size, d } = qrPath("http://192.168.1.10:5173");
  assert.ok(size >= 21 && (size - 17) % 4 === 0, `a real QR size: ${size}`);
  assert.match(d, /^M0 0h7v1h-7z/, "the top left finder pattern starts the path");
  assert.equal(qrPath("http://192.168.1.10:5173").d, d, "the same address draws the same code");
});

test("the phone opens the same page on the network address", () => {
  const urls = ["http://localhost:5173", "http://192.168.1.10:5173", "http://192.168.1.10:3000"];
  assert.equal(phoneFor("http://localhost:5173/shop?x=1", urls), "http://192.168.1.10:5173/shop?x=1");
  assert.equal(phoneFor("http://localhost:5173/", urls), "http://192.168.1.10:5173");
  assert.equal(phoneFor("http://localhost:8080/", urls), null, "no network address on that port");
  assert.equal(phoneFor("not a url", urls), null);
});
