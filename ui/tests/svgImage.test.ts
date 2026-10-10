import test from "node:test";
import assert from "node:assert/strict";
import { svgDataUrl, withNamespaces } from "../src/lib/svgImage.ts";

const decode = (url: string) => {
  const b64 = url.replace(/^data:image\/svg\+xml;base64,/, "");
  return new TextDecoder().decode(Uint8Array.from(atob(b64), (c) => c.charCodeAt(0)));
};

test("svg artifacts become a base64 image url, never inline markup", () => {
  const svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><style>body{display:none}</style><rect width="10" height="10"/></svg>';
  const url = svgDataUrl(svg);
  assert.match(url, /^data:image\/svg\+xml;base64,[A-Za-z0-9+/=]+$/);
  assert.equal(decode(url), svg);
});

test("unicode text survives the round trip", () => {
  const svg = '<svg xmlns="http://www.w3.org/2000/svg"><text>Grüße → 日本 ✓</text></svg>';
  assert.equal(decode(svgDataUrl(svg)), svg);
});

test("missing namespaces are added so the image decodes", () => {
  assert.equal(withNamespaces('<svg viewBox="0 0 1 1"></svg>'), '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"></svg>');
  const linked = withNamespaces('<svg><use xlink:href="#a"/></svg>');
  assert.match(linked, /xmlns="http:\/\/www\.w3\.org\/2000\/svg"/);
  assert.match(linked, /xmlns:xlink="http:\/\/www\.w3\.org\/1999\/xlink"/);
  const kept = '<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink"><use xlink:href="#a"/></svg>';
  assert.equal(withNamespaces(kept), kept);
});

test("non-svg content yields no image", () => {
  assert.equal(svgDataUrl("<div>hi</div>"), "");
  assert.equal(svgDataUrl(""), "");
});

test("large drawings encode without blowing the call stack", () => {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg">${"<rect/>".repeat(40000)}</svg>`;
  assert.equal(decode(svgDataUrl(svg)), svg);
});
