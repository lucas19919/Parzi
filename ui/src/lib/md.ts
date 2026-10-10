import MarkdownIt from "markdown-it";
import DOMPurify from "dompurify";
import { writable } from "svelte/store";
import { toast } from "./toast";

// Rich renderers (KaTeX + highlight.js) load on first use, off the
// startup path. Until then code renders plain and math renders as
// escaped source; `richReady` flips and cached fallbacks are dropped.
export const richReady = writable(false);

type Katex = { renderToString: (tex: string, opts: Record<string, unknown>) => string };
type Hljs = { getLanguage: (name: string) => unknown; highlight: (code: string, opts: { language: string }) => { value: string } };

let katex: Katex | null = null;
let hljs: Hljs | null = null;
let richPromise: Promise<void> | null = null;

export function ensureRich(): Promise<void> {
  if (!richPromise) {
    richPromise = (async () => {
      const [k, h, langs] = await Promise.all([
        import("katex"),
        import("highlight.js/lib/core"),
        Promise.all([
          import("highlight.js/lib/languages/rust"),
          import("highlight.js/lib/languages/python"),
          import("highlight.js/lib/languages/javascript"),
          import("highlight.js/lib/languages/typescript"),
          import("highlight.js/lib/languages/bash"),
          import("highlight.js/lib/languages/json"),
          import("highlight.js/lib/languages/ini"),
          import("highlight.js/lib/languages/xml"),
          import("highlight.js/lib/languages/css"),
          import("highlight.js/lib/languages/sql"),
          import("highlight.js/lib/languages/go"),
          import("highlight.js/lib/languages/markdown"),
          import("highlight.js/lib/languages/yaml"),
          import("highlight.js/lib/languages/diff"),
          import("highlight.js/lib/languages/c"),
          import("highlight.js/lib/languages/cpp"),
        ]),
        // KaTeX's stylesheet ships with the lazy chunk, not the startup CSS.
        import("katex/dist/katex.min.css"),
      ]);
      katex = ((k as any).default ?? k) as Katex;
      const core = ((h as any).default ?? h) as Hljs & { registerLanguage: (name: string, lang: unknown) => void };
      const [rust, python, javascript, typescript, bash, json, ini, xml, css, sql, go, markdown, yaml, diff, c, cpp] =
        langs.map((m) => (m as any).default ?? m);
      core.registerLanguage("rust", rust);
      core.registerLanguage("rs", rust);
      core.registerLanguage("python", python);
      core.registerLanguage("py", python);
      core.registerLanguage("javascript", javascript);
      core.registerLanguage("js", javascript);
      core.registerLanguage("typescript", typescript);
      core.registerLanguage("ts", typescript);
      core.registerLanguage("bash", bash);
      core.registerLanguage("sh", bash);
      core.registerLanguage("shell", bash);
      core.registerLanguage("zsh", bash);
      core.registerLanguage("json", json);
      core.registerLanguage("toml", ini);
      core.registerLanguage("ini", ini);
      core.registerLanguage("xml", xml);
      core.registerLanguage("html", xml);
      core.registerLanguage("svg", xml);
      core.registerLanguage("css", css);
      core.registerLanguage("sql", sql);
      core.registerLanguage("go", go);
      core.registerLanguage("golang", go);
      core.registerLanguage("markdown", markdown);
      core.registerLanguage("md", markdown);
      core.registerLanguage("yaml", yaml);
      core.registerLanguage("yml", yaml);
      core.registerLanguage("diff", diff);
      core.registerLanguage("patch", diff);
      core.registerLanguage("c", c);
      core.registerLanguage("cpp", cpp);
      hljs = core;
      mdCache.clear();
      richReady.set(true);
    })().catch((e) => {
      // Keep the settled promise: a missing chunk stays missing, so one
      // toast beats a failed import (and a retry) on every render.
      toast(`Code highlighting and math are off: ${e}`, true);
    });
  }
  return richPromise;
}

const md = new MarkdownIt({
  html: false,
  linkify: true,
});

const defaultValidateLink = md.validateLink.bind(md);
md.validateLink = (url: string) => {
  if (/^(?:file|brain|parzi):/i.test(url)) return true;
  return defaultValidateLink(url);
};

function texHtml(tex: string, display: boolean): string | null {
  if (!tex.trim() || !katex) return null;
  try {
    return katex.renderToString(tex, {
      displayMode: display,
      throwOnError: true,
      output: "html",
      strict: false,
    });
  } catch {
    return null;
  }
}

// $$…$$ display blocks (single- or multi-line).
md.block.ruler.before("fence", "parzi-math-block", (state, start, end, silent) => {
  const first = state.getLines(start, start + 1, 0, false).trim();
  if (!first.startsWith("$$")) return false;
  let line = start;
  let body = first.slice(2);
  if (body.includes("$$")) {
    body = body.slice(0, body.indexOf("$$"));
  } else {
    line++;
    const parts: string[] = [body];
    let closed = false;
    for (; line <= end; line++) {
      const text = state.getLines(line, line + 1, 0, false);
      const cut = text.indexOf("$$");
      if (cut >= 0) {
        parts.push(text.slice(0, cut));
        closed = true;
        break;
      }
      parts.push(text);
    }
    if (!closed) return false;
    body = parts.join("\n");
  }
  if (silent) return true;
  const tok = state.push("parzi_math", "div", 0);
  tok.content = body;
  tok.meta = { display: true };
  tok.map = [start, line + 1];
  state.line = line + 1;
  return true;
});

// $…$ inline and $$…$$ single-line display (escaped \$ ignored).
md.inline.ruler.after("escape", "parzi-math-inline", (state, silent) => {
  const src = state.src;
  if (state.pos >= src.length || src[state.pos] !== "$") return false;
  if (state.pos > 0 && src[state.pos - 1] === "\\") return false;
  const display = src[state.pos + 1] === "$";
  const open = display ? state.pos + 2 : state.pos + 1;
  let end = open;
  while (end < src.length) {
    if (src[end] === "\n") return false;
    if (src[end] === "$" && src[end - 1] !== "\\") {
      if (display) {
        if (src[end + 1] === "$") break;
      } else if (src[end + 1] !== "$") {
        break;
      }
    }
    end++;
  }
  if (end >= src.length) return false;
  const tex = src.slice(open, end);
  if (!tex.trim()) return false;
  if (silent) return true;
  const tok = state.push("parzi_math", "span", 0);
  tok.content = tex;
  tok.meta = { display };
  state.pos = display ? end + 2 : end + 1;
  return true;
});

md.renderer.rules.parzi_math = (tokens, idx) => {
  const tok = tokens[idx];
  const html = texHtml(tok.content, !!tok.meta?.display);
  if (html) return tok.meta?.display ? `<div class="md-math">${html}</div>` : html;
  const esc = tok.content.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return tok.meta?.display ? `$$${esc}$$` : `$${esc}$`;
};

const PLAIN_FENCES = new Set([
  "",
  "code",
  "text",
  "txt",
  "ascii",
  "tree",
  "console",
  "terminal",
  "log",
]);

// Set while LiveMarkdown renders a tail whose fence is still open: that
// block re-renders on every streamed chunk, so it stays escaped text and
// highlights once, after the fence closes.
let streamingFence = false;

md.renderer.rules.fence = (tokens, idx) => {
  const tok = tokens[idx];
  const lang = (tok.info || "").trim().split(/\s+/)[0] || "";
  const safeLang = lang.replace(/[^\w+#.-]/g, "");
  const body = tok.content.replace(/\n$/, "");
  const lines = body.split("\n");
  let code: string;
  if (safeLang === "diff" || safeLang === "patch") {
    code = lines
      .map((l) => {
        const cls = l.startsWith("+") && !l.startsWith("+++") ? " dl-add" : l.startsWith("-") && !l.startsWith("---") ? " dl-del" : "";
        return `<span class="dl-line${cls}">${escapeHtml(l) || " "}</span>`;
      })
      .join("\n");
  } else {
    const plain = streamingFence || !hljs || PLAIN_FENCES.has(lang.toLowerCase()) || !hljs.getLanguage(lang);
    try {
      code = plain || !hljs ? escapeHtml(body) : hljs.highlight(body, { language: lang }).value;
    } catch {
      code = escapeHtml(body);
    }
  }
  const label = PLAIN_FENCES.has(lang.toLowerCase()) ? "" : `<span class="code-lang">${safeLang}</span>`;
  const long = lines.length > 30;
  const expand = long ? `<button class="code-expand" data-expand data-label="Show all ${lines.length} lines">Show all ${lines.length} lines</button>` : "";
  return `<div class="codeblock${long ? " clamped" : ""}"><div class="code-tools">${label}<button data-copy>Copy</button></div><pre><code>${code}</code></pre>${expand}</div>`;
};

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

const TASK_MARK = /^\[([ xX])\]\s+/;

md.core.ruler.before("inline", "parzi-tasklists", (state) => {
  const toks = state.tokens;
  for (let i = 0; i < toks.length; i++) {
    const tok = toks[i];
    if (tok.type !== "inline") continue;
    const m = TASK_MARK.exec(tok.content);
    if (!m) continue;
    let j = i - 1;
    while (j >= 0 && toks[j].type === "paragraph_open") j--;
    if (j < 0 || toks[j].type !== "list_item_open") continue;
    const done = m[1] !== " ";
    tok.content = (done ? "☑ " : "☐ ") + tok.content.slice(m[0].length);
    toks[j].attrJoin("class", done ? "task done" : "task");
  }
});

const FRONT_LINE = /^([A-Za-z][A-Za-z0-9 _-]*):[ \t]+(\S.*)$/;

md.core.ruler.push("parzi-front", (state) => {
  const toks = state.tokens;
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

const mdCache = new Map<string, string>();
const MD_CACHE_MAX = 400;

const LOCAL_IMAGE = /^(data:image\/|asset:|https?:\/\/asset\.localhost\/)/i;

DOMPurify.addHook("afterSanitizeAttributes", (node) => {
  // HTML tagNames are upper-case, SVG ones (<image>) keep their case.
  const tag = (node.tagName ?? "").toLowerCase();
  if (tag === "a") {
    node.setAttribute("target", "_blank");
    node.setAttribute("rel", "noopener noreferrer");
  } else if (tag === "img" || tag === "image") {
    const refs = ["src", "href", "xlink:href"].map((a) => node.getAttribute(a)).filter((v): v is string => v !== null);
    if (node.hasAttribute("srcset") || !refs.length || refs.some((v) => !LOCAL_IMAGE.test(v))) {
      for (const a of ["src", "srcset", "href", "xlink:href"]) node.removeAttribute(a);
      node.setAttribute("title", "Remote image not loaded");
    }
  }
});

export function renderMarkdown(src: string, cache = true): string {
  // Kick the rich renderers off the startup path, and only for text that
  // can use them (a fence or math). The first render may fall back to
  // plain code / escaped math and upgrades when they land (callers pass
  // $richReady via mdHtml so markup re-renders then).
  if (src.includes("```") || src.includes("~~~") || src.includes("$")) void ensureRich();
  if (cache) {
    const hit = mdCache.get(src);
    if (hit !== undefined) {
      return hit;
    }
  }
  const res = DOMPurify.sanitize(md.render(src), {
    ALLOWED_URI_REGEXP: /^(?:(?:https?|parzi|brain|file):|[^a-z]|[a-z+.\-]+(?:[^a-z+.\-:]|$))/i,
  });
  if (!cache) return res;
  if (mdCache.size >= MD_CACHE_MAX) {
    const first = mdCache.keys().next().value;
    if (first !== undefined) mdCache.delete(first);
  }
  mdCache.set(src, res);
  return res;
}

// Reactive wrapper for markup: `{@html mdHtml(body, $richReady)}`
// re-renders with highlighting/math once the lazy chunks arrive.
export function mdHtml(src: string, _rev: unknown, cache = true): string {
  return renderMarkdown(src, cache);
}

type Segment =
  | { kind: "md"; body: string }
  | { kind: "widget"; body: unknown }
  | { kind: "artifact"; body: any };

const segCache = new Map<string, Segment[]>();
const SEG_CACHE_MAX = 400;

export function splitSegments(text: string, cache = true): Segment[] {
  if (cache) {
    const hit = segCache.get(text);
    if (hit !== undefined) return hit;
  }
  const out: Segment[] = [];
  const re = /```(parzi-widget|parzi-artifact)\n([\s\S]*?)```/g;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = re.exec(text)) !== null) {
    if (m.index > last) out.push({ kind: "md", body: text.slice(last, m.index) });
    try {
      const payload = JSON.parse(m[2]);
      if (m[1] === "parzi-widget") out.push({ kind: "widget", body: payload });
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

function continues(line: string): boolean {
  return /^(\s*([-*+]|\d+[.)])\s|\s{4,}|>|\||\s*\[[^\]]+\]:)/.test(line);
}

function stableCut(text: string): number {
  const lines = text.split("\n");
  let inFence = false;
  let mark = "";
  let cut = 0;
  let pos = 0;
  let lastNonBlank = "";
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

// True when `text` ends inside a ``` / ~~~ fence (the closing line may
// still be streaming in).
function fenceOpen(text: string): boolean {
  let mark = "";
  for (const line of text.split("\n")) {
    const f = /^(`{3,}|~{3,})/.exec(line.trim());
    if (!f) continue;
    if (!mark) mark = f[1][0];
    else if (f[1][0] === mark) mark = "";
  }
  return !!mark;
}

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
      }
      this.src = text.slice(0, cut);
    }
    const tail = text.slice(this.src.length);
    if (!tail.trim()) return { head: this.headHtml, tail: "" };
    streamingFence = fenceOpen(tail);
    try {
      return { head: this.headHtml, tail: renderMarkdown(tail, false) };
    } finally {
      streamingFence = false;
    }
  }

  reset(): void {
    this.src = "";
    this.headHtml = "";
  }
}
