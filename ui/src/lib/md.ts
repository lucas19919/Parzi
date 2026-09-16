import MarkdownIt from "markdown-it";
import DOMPurify from "dompurify";
import hljs from "highlight.js/lib/core";
import rust from "highlight.js/lib/languages/rust";
import python from "highlight.js/lib/languages/python";
import javascript from "highlight.js/lib/languages/javascript";
import typescript from "highlight.js/lib/languages/typescript";
import bash from "highlight.js/lib/languages/bash";
import json from "highlight.js/lib/languages/json";
import ini from "highlight.js/lib/languages/ini";
import xml from "highlight.js/lib/languages/xml";
import css from "highlight.js/lib/languages/css";
import sql from "highlight.js/lib/languages/sql";
import go from "highlight.js/lib/languages/go";
import markdown from "highlight.js/lib/languages/markdown";
import yaml from "highlight.js/lib/languages/yaml";
import diff from "highlight.js/lib/languages/diff";
import c from "highlight.js/lib/languages/c";
import cpp from "highlight.js/lib/languages/cpp";

hljs.registerLanguage("rust", rust);
hljs.registerLanguage("rs", rust);
hljs.registerLanguage("python", python);
hljs.registerLanguage("py", python);
hljs.registerLanguage("javascript", javascript);
hljs.registerLanguage("js", javascript);
hljs.registerLanguage("typescript", typescript);
hljs.registerLanguage("ts", typescript);
hljs.registerLanguage("bash", bash);
hljs.registerLanguage("sh", bash);
hljs.registerLanguage("shell", bash);
hljs.registerLanguage("zsh", bash);
hljs.registerLanguage("json", json);
hljs.registerLanguage("toml", ini);
hljs.registerLanguage("ini", ini);
hljs.registerLanguage("xml", xml);
hljs.registerLanguage("html", xml);
hljs.registerLanguage("svg", xml);
hljs.registerLanguage("css", css);
hljs.registerLanguage("sql", sql);
hljs.registerLanguage("go", go);
hljs.registerLanguage("golang", go);
hljs.registerLanguage("markdown", markdown);
hljs.registerLanguage("md", markdown);
hljs.registerLanguage("yaml", yaml);
hljs.registerLanguage("yml", yaml);
hljs.registerLanguage("diff", diff);
hljs.registerLanguage("patch", diff);
hljs.registerLanguage("c", c);
hljs.registerLanguage("cpp", cpp);


const md = new MarkdownIt({
  html: false,
  linkify: true,
  highlight: (code, lang) => {
    try {
      if (lang && hljs.getLanguage(lang)) {
        return hljs.highlight(code, { language: lang }).value;
      }
      // Plain-text / ASCII art: never auto-highlight. highlightAuto injects
      // random spans that break box-drawing alignment (see arch-diagram bug).
      return escapeHtml(code);
    } catch {
      return "";
    }
  },
});

// Fences that are really ASCII diagrams / plain text. Never syntax-tint these:
// the tint spans split box-drawing chars and destroy column alignment.
const PLAIN_FENCES = new Set([
  "",
  "code",
  "text",
  "txt",
  "ascii",
  "diagram",
  "tree",
  "console",
  "terminal",
  "log",
]);

/** Fenced blocks: header (ext badge + lines + copy), gutter numbers,
    diff tinting, auto-clamp past 30 lines with expander. */
md.renderer.rules.fence = (tokens, idx) => {
  const tok = tokens[idx];
  const lang = (tok.info || "").trim().split(/\s+/)[0] || "code";
  const safeLang = lang.replace(/[<>&"]/g, "");
  const lines = tok.content.replace(/\n$/, "").split("\n");
  const isDiff = safeLang === "diff";
  let code: string;
  if (isDiff) {
    const badge = (l: string) =>
      l.startsWith("+") && !l.startsWith("+++")
        ? ["dl-add", "+"]
        : l.startsWith("-") && !l.startsWith("---")
          ? ["dl-del", "−"]
          : ["", " "];
    code = lines
      .map((l) => {
        const [cls, mark] = badge(l);
        return `<span class="dl-line ${cls}"><span class="dl-mark">${mark}</span><span>${escapeHtml(l) || " "}</span></span>`;
      })
      .join("\n");
  } else {
    let html: string;
    const plain = PLAIN_FENCES.has(lang.toLowerCase()) || !hljs.getLanguage(lang);
    try {
      html =
        !plain && lang && hljs.getLanguage(lang)
          ? hljs.highlight(tok.content, { language: lang }).value
          : escapeHtml(tok.content);
    } catch {
      html = escapeHtml(tok.content);
    }
    const htmlLines = balanceLines(html);
    code = htmlLines
      .map((l, i) => `<span class="cl-line"><span class="cl-no">${i + 1}</span><span class="cl-tx">${stripLeadingBullet(l, i + 1) || " "}</span></span>`)
      .join("\n");
  }
  const clamped = lines.length > 30 ? " clamped" : "";
  const expand =
    lines.length > 30
      ? `<button class="code-expand" data-expand data-label="Show full snippet (${lines.length} lines)">Show full snippet (${lines.length} lines)</button>`
      : "";
  return `<div class="codeblock${clamped}"><div class="code-head"><span class="ext">${safeLang}</span><span class="lc">${lines.length} lines</span><button data-copy>copy</button></div><pre><code>${code}</code></pre>${expand}</div>`;
};

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

/** Split highlighted HTML into per-line fragments with spans balanced.
    hljs opens a <span> on one line and closes it lines later (multi-line
    comments, <style>/<script> runs, template literals). Naive splitting
    lets the open span swallow the following rows' gutter markup, so gutters
    jump (1-6, 19-24, …) and rows merge. Close every open span at EOL and
    reopen on the next line. */
function balanceLines(html: string): string[] {
  const raw = html.replace(/\n$/, "").split("\n");
  const out: string[] = [];
  let open: string[] = [];
  const re = /<(\/?)span(\s[^>]*)?>/g;
  for (const line of raw) {
    const prev = open.join("");
    re.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = re.exec(line)) !== null) {
      if (m[1] === "/") open.pop();
      else open.push(m[0]);
    }
    out.push(prev + (line || " ") + "</span>".repeat(open.length));
  }
  return out;
}

/** Drop a highlight bullet that only duplicates the gutter number: `-`/`*`/`+`
    always; `N.`/`N)` only when N equals the gutter line (a list starting at
    0. or 3. keeps its marker — the gutter must not rewrite content). */
function stripLeadingBullet(line: string, gutter: number): string {
  const m = /^<span class="hljs-bullet">(.*?)<\/span>/.exec(line);
  if (!m) return line;
  const mark = m[1].trim();
  if (mark === "-" || mark === "*" || mark === "+") return line.slice(m[0].length);
  const n = /^(\d+)[.)]$/.exec(mark);
  if (n && Number(n[1]) === gutter) return line.slice(m[0].length);
  return line;
}
/** GFM-ish task lists without a plugin: `- [ ]` / `- [x]` rendering.
    It has to run *before* `inline`: after that rule the renderer draws the
    token's `children`, not its `content`, so rewriting `content` there was
    thrown away and PROJECT.md showed a literal `[x]`. Only the first
    paragraph of a list item counts, so `[x]` in prose stays prose. */
const TASK_MARK = /^\[([ xX])\]\s+/;

md.core.ruler.before("inline", "parzi-tasklists", (state) => {
  const toks = state.tokens;
  for (let i = 0; i < toks.length; i++) {
    const tok = toks[i];
    if (tok.type !== "inline") continue;
    const m = TASK_MARK.exec(tok.content);
    if (!m) continue;
    // Tight lists hide the paragraph_open; loose lists keep it. Either way
    // the task line is the paragraph that opens a list item.
    let j = i - 1;
    while (j >= 0 && toks[j].type === "paragraph_open") j--;
    if (j < 0 || toks[j].type !== "list_item_open") continue;
    const done = m[1] !== " ";
    tok.content = (done ? "☑ " : "☐ ") + tok.content.slice(m[0].length);
    toks[j].attrJoin("class", done ? "task done" : "task");
  }
});

/** The `key: value` block at the head of PROJECT.md is a definition list,
    not prose: markdown-it glues those lines into one paragraph and the
    reader loses every field boundary. Only the document's first paragraph
    qualifies, and only when every line of it is a field. */
const FRONT_LINE = /^([A-Za-z][A-Za-z0-9 _-]*):[ \t]+(\S.*)$/;

md.core.ruler.push("parzi-front", (state) => {
  const toks = state.tokens;
  // Nothing but an opening heading may come first.
  let at = 0;
  if (toks[at]?.type === "heading_open") at += 3;
  if (toks[at]?.type !== "paragraph_open" || toks[at + 1]?.type !== "inline") return;
  const lines = toks[at + 1].content.split("\n");
  if (lines.length < 2 || !lines.every((l) => FRONT_LINE.test(l))) return;
  const front = new state.Token("parzi_front", "", 0);
  front.content = toks[at + 1].content;
  toks.splice(at, 3, front);
});

md.renderer.rules.parzi_front = (tokens, idx) => {
  const rows = tokens[idx].content
    .split("\n")
    .map((l) => FRONT_LINE.exec(l))
    .filter((m): m is RegExpExecArray => !!m)
    .map((m) => `<dt>${escapeHtml(m[1])}</dt><dd>${escapeHtml(m[2])}</dd>`)
    .join("");
  return `<dl class="md-front">${rows}</dl>`;
};

const DEV: boolean = import.meta.env?.DEV ?? true;

/** Dev-only counters for the streaming path: `parses` is work actually done,
    `hits` is work the LRU saved. Exposed as `window.__parziMd` in dev and read
    by `tests/stream-bench.ts`. Cheap increments; never read in production. */
export const mdStats = {
  renders: 0,
  parses: 0,
  hits: 0,
  parsedChars: 0,
  splits: 0,
  splitParses: 0,
  liveBlocks: 0,
};
export function resetMdStats(): void {
  for (const k of Object.keys(mdStats) as (keyof typeof mdStats)[]) mdStats[k] = 0;
}
if (DEV) (globalThis as Record<string, unknown>).__parziMd = mdStats;

const mdCache = new Map<string, string>();
const MD_CACHE_MAX = 400;

/** `cache` is false for the streaming tail: its source string changes on every
    flush, so caching it would evict every finished message within one answer
    (400 entries, ~270 flushes for a 4 000-token reply). */
export function renderMarkdown(src: string, cache = true): string {
  if (DEV) mdStats.renders++;
  if (cache) {
    const hit = mdCache.get(src);
    if (hit !== undefined) {
      if (DEV) mdStats.hits++;
      return hit;
    }
  }
  if (DEV) {
    mdStats.parses++;
    mdStats.parsedChars += src.length;
  }
  const res = DOMPurify.sanitize(md.render(src), {
    ALLOWED_URI_REGEXP: /^(?:(?:https?|parzi):|[^a-z]|[a-z+.-]+(?:[^a-z+.\-:]|$))/i,
  });
  if (!cache) return res;
  if (mdCache.size >= MD_CACHE_MAX) {
    const first = mdCache.keys().next().value;
    if (first !== undefined) mdCache.delete(first);
  }
  mdCache.set(src, res);
  return res;
}

export type Segment =
  | { kind: "md"; body: string }
  | { kind: "widget"; body: unknown }
  | { kind: "diagram"; body: unknown }
  | { kind: "artifact"; body: any };

const segCache = new Map<string, Segment[]>();
const SEG_CACHE_MAX = 400;

/** Split ```parzi-widget / ```parzi-diagram / ```parzi-artifact fences out for
    component rendering. `cache` is false for the growing live buffer (see
    `renderMarkdown`); the scan itself is a single regex pass. */
export function splitSegments(text: string, cache = true): Segment[] {
  if (DEV) mdStats.splits++;
  if (cache) {
    const hit = segCache.get(text);
    if (hit !== undefined) return hit;
  }
  if (DEV) mdStats.splitParses++;
  const out: Segment[] = [];
  const re = /```(parzi-widget|parzi-diagram|parzi-artifact)\n([\s\S]*?)```/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) out.push({ kind: "md", body: text.slice(last, m.index) });
    try {
      const payload = JSON.parse(m[2]);
      if (m[1] === "parzi-widget") out.push({ kind: "widget", body: payload });
      else if (m[1] === "parzi-diagram") out.push({ kind: "diagram", body: payload });
      else out.push({ kind: "artifact", body: payload });
    } catch {
      out.push({ kind: "md", body: m[0] });
    }
    last = m.index + m[0].length;
  }
  if (last < text.length) out.push({ kind: "md", body: text.slice(last) });
  if (!cache) return out;
  if (segCache.size >= SEG_CACHE_MAX) {
    const first = segCache.keys().next().value;
    if (first !== undefined) segCache.delete(first);
  }
  segCache.set(text, out);
  return out;
}

/** A line that a following blank line does not necessarily end: list items,
    block quotes, table rows and indented code can all resume after one. */
function continues(line: string): boolean {
  return /^(\s*([-*+]|\d+[.)])\s|\s{4,}|>|\||\s*\[[^\]]+\]:)/.test(line);
}

/** Index just past the last block boundary that can never change again: a
    blank line outside a fence whose preceding block cannot be continued. */
function stableCut(text: string): number {
  const lines = text.split("\n");
  let inFence = false;
  let mark = "";
  let cut = 0;
  let pos = 0;
  let lastNonBlank = "";
  // The final line is whatever the model is typing right now: never freeze it.
  for (let i = 0; i < lines.length - 1; i++) {
    const line = lines[i];
    const end = pos + line.length + 1;
    const t = line.trim();
    const f = /^(`{3,}|~{3,})/.exec(t);
    if (f) {
      if (!inFence) {
        inFence = true;
        mark = f[1][0];
      } else if (f[1][0] === mark) {
        inFence = false;
      }
    } else if (!inFence && t === "" && lastNonBlank && !continues(lastNonBlank)) {
      cut = end;
    }
    if (t !== "") lastNonBlank = line;
    pos = end;
  }
  return cut;
}

/** Incremental renderer for the streaming tail (E4). Blocks that are finished
    are parsed once and their HTML is kept; each flush only parses the block
    still being written. Per flush the cost is the open block, not the whole
    answer, which is what made the old path quadratic. One instance per live
    view; it re-seeds itself when the text is not an extension of what it has. */
export class LiveMarkdown {
  private src = "";
  private headHtml = "";

  render(text: string): { head: string; tail: string } {
    if (!text.startsWith(this.src)) this.reset();
    const cut = stableCut(text);
    if (cut > this.src.length) {
      const chunk = text.slice(this.src.length, cut);
      if (chunk.trim()) {
        this.headHtml += renderMarkdown(chunk, false);
        if (DEV) mdStats.liveBlocks++;
      }
      this.src = text.slice(0, cut);
    }
    const tail = text.slice(this.src.length);
    return { head: this.headHtml, tail: tail.trim() ? renderMarkdown(tail, false) : "" };
  }

  reset(): void {
    this.src = "";
    this.headHtml = "";
  }
}
