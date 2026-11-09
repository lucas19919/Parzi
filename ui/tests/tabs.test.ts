import { test } from "node:test";
import assert from "node:assert/strict";
import { addressKind, ensureComposer, folderName, hostOf, isExplicitUrl, placeTab, sessionTab, pageTab, toAddress, restoreTab } from "../src/lib/tabs.ts";

test("hostOf strips protocol and path", () => {
  assert.equal(hostOf("https://github.com/lucas/parzi"), "github.com");
  assert.equal(hostOf("https://www.example.com/a?b#c"), "example.com");
  assert.equal(hostOf(""), "");
});

test("hostOf keeps the port and drops sign-in details", () => {
  assert.equal(hostOf("http://localhost:5173/app"), "localhost:5173");
  assert.equal(hostOf("https://user:secret@example.com/"), "example.com");
  assert.equal(hostOf("https://a-very-long-subdomain.of-some-company.example.co.uk/"), "a-very-long-subdomain.of-some-company.example.co.uk");
  assert.equal(hostOf("about:blank"), "");
  assert.equal(hostOf("not a url"), "");
});

test("toAddress builds https or search fallback", () => {
  assert.equal(toAddress("https://example.com/x"), "https://example.com/x");
  assert.equal(toAddress("example.com"), "https://example.com");
  assert.equal(toAddress("hello world"), "https://duckduckgo.com/?q=hello%20world");
});

test("isExplicitUrl is strict", () => {
  assert.equal(isExplicitUrl("https://example.com/x"), true);
  assert.equal(isExplicitUrl("example.com"), false);
  assert.equal(isExplicitUrl("hello world"), false);
});

test("folderName takes the leaf", () => {
  assert.equal(folderName("C:\\src\\parzi"), "parzi");
  assert.equal(folderName("/home/u/app/"), "app");
});

test("a composer always has quote and attachment lists", () => {
  const fresh = ensureComposer();
  assert.deepEqual(fresh.quotes, []);
  assert.deepEqual(fresh.attachments, []);
  const saved = { input: "hi", mode: "work" as unknown as "build", model: "", effort: "medium", attachments: ["a.png"] };
  const partial = ensureComposer(saved);
  assert.equal(partial, saved);
  assert.equal(partial.input, "hi");
  assert.equal(partial.mode, "build");
  assert.deepEqual(partial.attachments, ["a.png"]);
  assert.deepEqual(partial.quotes, []);
  const kept = [{ project: "parzi", id: "prs", title: "PR badges" }];
  const quoted = ensureComposer({ quotes: kept, attachments: [] });
  assert.equal(quoted.quotes, kept);
  const ready = ensureComposer({ input: "", mode: "build", model: "", effort: "medium", attachments: [], quotes: [] });
  assert.equal(ensureComposer(ready), ready);
  assert.deepEqual(sessionTab().composer.quotes, []);
  assert.deepEqual(sessionTab().composer.attachments, []);
});

test("saved Work, Tasks, Brain, Projects and History tabs don't come back", () => {
  assert.equal(restoreTab({ id: "t1", kind: "tasks", title: "Tasks" }), null);
  assert.equal(restoreTab({ id: "t2", kind: "work", title: "Work" }), null);
  assert.equal(restoreTab({ id: "t5", kind: "brain", title: "Brain" }), null);
  assert.equal(restoreTab({ id: "t6", kind: "projects", title: "Projects" }), null);
  assert.equal(restoreTab({ id: "t7", kind: "history", title: "History" }), null);
  assert.equal(restoreTab({ id: "t3", kind: "settings", title: "Settings" }), null);
  assert.equal(restoreTab({ id: "t4", kind: "mystery", title: "?" }), null);
});

test("a new tab goes after its session's last tab, else right after the one named", () => {
  const a = { ...sessionTab("s1", "One"), id: "a" };
  const ap = { ...pageTab("https://x.dev/"), id: "ap", group: "s1" };
  const b = { ...sessionTab("s2", "Two"), id: "b" };
  const t = { ...pageTab("https://y.dev/"), id: "t" };
  const ids = (l: { id: string }[]) => l.map((x) => x.id);
  assert.deepEqual(ids(placeTab([a, ap, b], t, "b", "s1")), ["a", "ap", "t", "b"]);
  assert.deepEqual(ids(placeTab([a, ap, b], t, "a")), ["a", "t", "ap", "b"]);
  assert.deepEqual(ids(placeTab([a, ap, b], t, "a", "nobody")), ["a", "t", "ap", "b"], "an unknown group falls back to after");
  assert.deepEqual(ids(placeTab([a, b], t, "gone")), ["t", "a", "b"]);
});

test("the address box decides once what it holds", () => {
  assert.equal(addressKind("  "), "empty");
  assert.equal(addressKind("https://parzi.dev/x"), "full");
  assert.equal(addressKind("localhost:5173/app"), "local");
  assert.equal(addressKind("docs.rs/serde"), "domain");
  assert.equal(addressKind("how to center a div"), "search");
  assert.equal(toAddress("localhost:5173"), "http://localhost:5173");
  assert.equal(toAddress("docs.rs"), "https://docs.rs");
  assert.equal(toAddress("rust lifetimes"), "https://duckduckgo.com/?q=rust%20lifetimes");
});
