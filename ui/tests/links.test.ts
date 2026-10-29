import test from "node:test";
import assert from "node:assert/strict";
import { fileLink, safeDecode, vaultRefs, webLink } from "../src/lib/linkRules.ts";

test("safeDecode survives malformed escapes", () => {
  assert.equal(safeDecode("a%20b"), "a b");
  assert.equal(safeDecode("100%"), "100%");
  assert.equal(safeDecode("%E0%A4%A"), "%E0%A4%A");
});

test("local file links open after confirm, with the full path", () => {
  assert.deepEqual(fileLink("file:///C:/Users/me/report.pdf"), { kind: "open", path: "C:\\Users\\me\\report.pdf" });
  assert.deepEqual(fileLink("file://C:/Users/me/a%20b.txt#L10-L20"), { kind: "open", path: "C:\\Users\\me\\a b.txt" });
  assert.deepEqual(fileLink("file://localhost/C:/x/y.png"), { kind: "open", path: "C:\\x\\y.png" });
  assert.deepEqual(fileLink("file:///home/me/notes.txt"), { kind: "open", path: "/home/me/notes.txt" });
  assert.deepEqual(fileLink("file:///C:/bad%E0%A4%A.txt"), { kind: "open", path: "C:\\bad%E0%A4%A.txt" });
});

test("UNC and network file links are refused", () => {
  for (const href of [
    "file:////server/share/x.txt",
    "file://server/share/x.txt",
    "file://%5C%5Cserver%5Cshare%5Cx.txt",
    "file:///%5C%5Cserver%5Cshare",
    "file:///\\\\server\\share\\x.txt",
    "file:////?/C:/x.txt",
  ]) {
    assert.equal(fileLink(href).kind, "refuse", href);
  }
});

test("executables and scripts are refused", () => {
  for (const href of [
    "file:///C:/x/setup.exe",
    "file:///C:/x/run.BAT",
    "file:///C:/x/a.ps1",
    "file:///C:/x/a.js",
    "file:///C:/x/a.lnk",
    "file:///C:/x/a.hta",
    "file:///C:/x/a.reg",
    "file:///C:/x/a.cpl",
    "file:///C:/x/run.exe.",
    "file:///C:/x/run.exe%20",
    "file:///C:/x/run.cmd%20.",
    "file:///C:/x/a.txt:evil.exe",
    "file:///C:/x/a.txt:stream",
  ]) {
    assert.equal(fileLink(href).kind, "refuse", href);
  }
  assert.equal(fileLink("file:///C:/x/a.json").kind, "open");
  assert.equal(fileLink("file:///C:/x/a.exe.txt").kind, "open");
});

test("project markdown file links route to notes", () => {
  assert.deepEqual(fileLink("file:///C:/Users/me/.parzi/brain/notes/a.md"), { kind: "note", path: "notes/a.md" });
  assert.deepEqual(fileLink("file:///C:/vault/projects/x/GOALS.md"), { kind: "note", path: "projects/x/GOALS.md" });
  assert.deepEqual(fileLink("file://notes/a.md"), { kind: "note", path: "notes/a.md" });
  assert.deepEqual(fileLink("file:notes/a.md"), { kind: "note", path: "notes/a.md" });
  assert.equal(fileLink("file://server/share/readme.txt").kind, "refuse");
  assert.deepEqual(fileLink("file:///C:/elsewhere/readme.md"), { kind: "open", path: "C:\\elsewhere\\readme.md" });
});

test("bare vault notes in prose are found as link targets", () => {
  assert.deepEqual(vaultRefs("see projects/acme/GOALS.md for the plan"), [
    { path: "projects/acme/GOALS.md", index: 4, length: 22 },
  ]);
  assert.deepEqual(vaultRefs("a is notes/a.md, b is widgets/w.md"), [
    { path: "notes/a.md", index: 5, length: 10 },
    { path: "widgets/w.md", index: 22, length: 12 },
  ]);
  assert.deepEqual(vaultRefs("(see notes/a.md), then notes/b.md."), [
    { path: "notes/a.md", index: 5, length: 10 },
    { path: "notes/b.md", index: 23, length: 10 },
  ]);
  assert.deepEqual(vaultRefs("at projects/acme/plan.md:12 review"), [
    { path: "projects/acme/plan.md:12", index: 3, length: 24 },
  ]);
  assert.deepEqual(vaultRefs("BRAIN://Notes/A.MD"), [{ path: "BRAIN://Notes/A.MD", index: 0, length: 18 }]);
});

test("vault refs ignore code ticks, links-in-words and plain words", () => {
  assert.deepEqual(vaultRefs("`notes/a.md`"), []);
  assert.deepEqual(vaultRefs("**notes/a.md**"), [{ path: "notes/a.md", index: 2, length: 10 }]);
  assert.deepEqual(vaultRefs("footnotes/foo.md"), []);
  assert.deepEqual(vaultRefs("release notes for fun"), []);
  assert.deepEqual(vaultRefs("no refs here.txt"), []);
});

test("web links: plain http upgrades, local http opens in a tab", () => {
  assert.deepEqual(webLink("https://example.com/a?b=1"), { kind: "open", url: "https://example.com/a?b=1" });
  assert.deepEqual(webLink("http://example.com/a"), { kind: "open", url: "https://example.com/a" });
  assert.deepEqual(webLink("HTTP://Example.com"), { kind: "open", url: "https://Example.com" });
  for (const href of ["http://localhost:5173/", "http://127.0.0.1:8080", "http://[::1]:3000/x", "http://app.localhost/", "http://user@localhost/"]) {
    const link = webLink(href);
    assert.equal(link.kind, "tab", href);
    if (link.kind === "tab") assert.ok(link.url.startsWith("http://"), link.url);
  }
  assert.equal(webLink("https://localhost:8443/").kind, "open");
});

test("web links never hand the backend a character it rejects", () => {
  const link = webLink("https://en.wikipedia.org/wiki/Rust_(programming_language)");
  assert.deepEqual(link, { kind: "open", url: "https://en.wikipedia.org/wiki/Rust_%28programming_language%29" });
  const odd = webLink("https://x.test/a'b*c$d;e|f^g\"h<i>j`k\\l");
  assert.equal(odd.kind, "open");
  if (odd.kind === "open") assert.doesNotMatch(odd.url, /[\\"<>`|^$;'()*]/);
  assert.equal(webLink("https://x.test/a b").kind, "refuse");
  assert.equal(webLink("https://").kind, "refuse");
  assert.equal(webLink(`https://x.test/${"a".repeat(9000)}`).kind, "refuse");
  assert.equal(webLink("javascript:alert(1)").kind, "refuse");
});
