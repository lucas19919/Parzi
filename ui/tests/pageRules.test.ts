import { test } from "node:test";
import assert from "node:assert/strict";
import {
  appFor,
  downloadNote,
  downloadShare,
  formatBytes,
  iconFor,
  originOf,
  permissionName,
  permissionText,
  popClosed,
  pushClosed,
  rememberIcon,
  tipSpot,
} from "../src/lib/pageRules.ts";

test("closed tabs keep the newest last, once per address, web pages only", () => {
  let list = pushClosed([], { url: "https://a.com/", title: "A", at: 1 });
  list = pushClosed(list, { url: "https://b.com/", title: "B", at: 2 });
  list = pushClosed(list, { url: "https://a.com/", title: "A again", at: 3 });
  assert.deepEqual(
    list.map((c) => c.title),
    ["B", "A again"],
  );
  assert.equal(pushClosed(list, { url: "about:blank", title: "", at: 4 }), list);
  const many = Array.from({ length: 30 }, (_, i) => ({ url: `https://s${i}.com/`, title: "", at: i }));
  assert.equal(many.reduce((l, c) => pushClosed(l, c, 25), [] as typeof many).length, 25);
  const [last, rest] = popClosed(list);
  assert.equal(last?.title, "A again");
  assert.deepEqual(rest.map((c) => c.title), ["B"]);
  assert.deepEqual(popClosed([]), [null, []]);
});

test("favicons: a reported icon wins, else the site's /favicon.ico", () => {
  assert.equal(originOf("https://GitHub.com/a/b?c#d"), "https://github.com");
  assert.equal(originOf("mailto:x@y.z"), "");
  assert.equal(iconFor({}, "https://github.com/x"), "https://github.com/favicon.ico");
  assert.equal(iconFor({}, "about:blank"), "");
  const icons = rememberIcon({}, "https://github.com/x", "https://github.githubassets.com/favicons/favicon.svg");
  assert.ok(icons);
  assert.equal(iconFor(icons!, "https://github.com/other"), "https://github.githubassets.com/favicons/favicon.svg");
  assert.equal(rememberIcon(icons!, "https://github.com/", "https://github.githubassets.com/favicons/favicon.svg"), null, "unchanged");
  assert.equal(rememberIcon({}, "https://a.com/", "http://a.com/i.png"), null, "the shell loads https only");
  assert.ok(rememberIcon({}, "https://a.com/", "data:image/png;base64,AAAA"));
  assert.equal(rememberIcon({}, "https://a.com/", `data:image/png;base64,${"A".repeat(9000)}`), null, "too big");
  let capped: Record<string, string> = {};
  for (let i = 0; i < 5; i++) capped = rememberIcon(capped, `https://s${i}.com/`, `https://s${i}.com/i.png`, 3) ?? capped;
  assert.deepEqual(Object.keys(capped), ["https://s2.com", "https://s3.com", "https://s4.com"]);
});

test("prompts name the permission and the app in plain words", () => {
  assert.equal(permissionText("camera"), "use your camera");
  assert.equal(permissionText("teleport"), "use a browser feature");
  assert.equal(permissionName("geolocation"), "Location");
  assert.equal(appFor("zoommtg://zoom.us/join?confno=1"), "Zoom");
  assert.equal(appFor("mailto:a@b.c"), "your email app");
  assert.equal(appFor("foo+bar:thing"), "the app for foo+bar links");
});

test("downloads read as sizes and states", () => {
  assert.equal(formatBytes(0), "0 B");
  assert.equal(formatBytes(512), "512 B");
  assert.equal(formatBytes(1536), "1.5 KB");
  assert.equal(formatBytes(20 * 1024 * 1024), "20 MB");
  assert.equal(downloadNote({ state: "progress", received: 1024, total: 4096 }), "1.0 KB of 4.0 KB");
  assert.equal(downloadNote({ state: "progress", received: 2048, total: 0 }), "2.0 KB");
  assert.equal(downloadNote({ state: "done", received: 10, total: 10 }), "10 B");
  assert.equal(downloadNote({ state: "canceled", received: 1, total: 10 }), "Cancelled");
  assert.equal(downloadShare({ state: "progress", received: 1, total: 4 }), 0.25);
  assert.equal(downloadShare({ state: "progress", received: 1, total: 0 }), undefined);
  assert.equal(downloadShare({ state: "done", received: 4, total: 4 }), undefined);
});

test("tooltips stay off the page: below, else above, else beside", () => {
  const view = { width: 1000, height: 800 };
  const tip = { width: 80, height: 24 };
  const toolbarButton = { left: 100, top: 78, right: 130, bottom: 108 };
  assert.deepEqual(tipSpot(toolbarButton, tip, view, null), { left: 75, top: 114 });
  const page = { left: 0, top: 118, right: 1000, bottom: 800 };
  assert.deepEqual(tipSpot(toolbarButton, tip, view, page), { left: 75, top: 48 });
  const topButton = { left: 100, top: 4, right: 130, bottom: 34 };
  const below = { left: 0, top: 36, right: 1000, bottom: 800 };
  const spot = tipSpot(topButton, tip, view, below);
  assert.equal(spot.left, 136);
  assert.ok(tipSpot({ left: 980, top: 10, right: 1000, bottom: 30 }, tip, view, null).left <= 1000 - 80 - 6);
});
