import { test } from "node:test";
import assert from "node:assert/strict";
import MarkdownIt from "markdown-it";
import { reportMarkdown } from "../src/lib/mdReport.ts";

const md = new MarkdownIt({ linkify: true });
reportMarkdown(md);

const TABLE = [
  "| Company | Revenue ($B) | Growth (FY25A) | Op. margin |",
  "| --- | --- | --- | --- |",
  "| Visa | 40.0 | 11.3% | 60.0% |",
  "| American Express | 72.2 | 9.5% | n.m. |",
  "| Mean | 56.1 | 10.4% | 60.0% |",
  "",
  "*Visa's fiscal year ends Sep 30.*",
].join("\n");

test("number columns align right, words stay left", () => {
  const html = md.render(TABLE);
  assert.match(html, /<div class="md-table"><table>/);
  assert.match(html, /<td>Visa<\/td>/);
  assert.match(html, /<td class="num">40\.0<\/td>/);
  assert.match(html, /<td class="num">11\.3%<\/td>/);
  assert.match(html, /<td class="num na">n\.m\.<\/td>/);
});

test("a header's unit goes under its name", () => {
  const html = md.render(TABLE);
  assert.match(html, /Revenue<span class="th-sub">\$B<\/span><\/th>/);
  assert.match(html, /Growth<span class="th-sub">FY25A<\/span><\/th>/);
  assert.match(html, /Op\. margin<\/th>/);
});

test("summary rows and the note under the table are marked", () => {
  const html = md.render(TABLE);
  assert.match(html, /<tr class="summary">\s*<td>Mean<\/td>/);
  assert.match(html, /<p class="md-note"><em>Visa/);
});

test("a numbered link is a source chip named for its site", () => {
  const html = md.render("Visa grew 11.3% [1](https://www.sec.gov/visa-10k) and that is that.");
  assert.match(html, /<a href="https:\/\/www\.sec\.gov\/visa-10k" class="cite" title="sec\.gov">1<\/a>/);
  const plain = md.render("See [the filing](https://sec.gov/x).");
  assert.doesNotMatch(plain, /cite/);
});
