import MarkdownIt from "markdown-it";
import DOMPurify from "dompurify";
import katex from "katex";
import "katex/dist/katex.min.css";
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
});

function texHtml(tex: string, display: boolean): string | null {
  if (!tex.trim()) return null;
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
    const plain = PLAIN_FENCES.has(lang.toLowerCase()) || !hljs.getLanguage(lang);
    try {
      code = plain ? escapeHtml(body) : hljs.highlight(body, { language: lang }).value;
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

const DEV: boolean = import.meta.env?.DEV ?? true;

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

const mdCache = new Map<string, string>();
const MD_CACHE_MAX = 400;

DOMPurify.addHook("afterSanitizeAttributes", (node) => {
  if (node.tagName === "A") {
    node.setAttribute("target", "_blank");
    node.setAttribute("rel", "noopener noreferrer");
  } else if (node.tagName === "IMG" && !/^(data:image\/|asset:|https?:\/\/asset\.localhost\/)/i.test(node.getAttribute("src") ?? "")) {
    node.removeAttribute("src");
    node.removeAttribute("srcset");
    node.setAttribute("title", "Remote image not loaded");
  }
});

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

type Segment =
  | { kind: "md"; body: string }
  | { kind: "widget"; body: unknown }
  | { kind: "artifact"; body: any };

const segCache = new Map<string, Segment[]>();
const SEG_CACHE_MAX = 400;

export function splitSegments(text: string, cache = true): Segment[] {
  if (DEV) mdStats.splits++;
  if (cache) {
    const hit = segCache.get(text);
    if (hit !== undefined) return hit;
  }
  if (DEV) mdStats.splitParses++;
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
