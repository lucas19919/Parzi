import MarkdownIt from "markdown-it";
import DOMPurify from "dompurify";
import { writable } from "svelte/store";
import { toast } from "./toast";
import { splitSegments, type Segment } from "./segments";
import { reportMarkdown } from "./mdReport";
import { mathAt } from "./mdMath";
import { vaultRefs } from "./linkRules";

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

md.inline.ruler.after("escape", "parzi-math-inline", (state, silent) => {
  const span = mathAt(state.src, state.pos);
  if (!span) return false;
  if (silent) return true;
  const tok = state.push("parzi_math", "span", 0);
  tok.content = span.tex;
  tok.meta = { display: span.display };
  state.pos = span.next;
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
  const cols = isDiagram(lang, body) ? Math.max(...lines.map((l) => [...l].length)) : 0;
  const long = !cols && lines.length > 30;
  const expand = long ? `<button class="code-expand" data-expand data-label="Show all ${lines.length} lines">Show all ${lines.length} lines</button>` : "";
  const cls = cols ? " diagram" : long ? " clamped" : "";
  const style = cols ? ` style="--cols:${cols}"` : "";
  return `<div class="codeblock${cls}"${style}><div class="code-tools">${label}<button data-copy>Copy</button></div><pre><code>${code}</code></pre>${expand}</div>`;
};

const BOX = /[─-╿▀-▟]/g;

export function isDiagram(lang: string, body: string): boolean {
  if (!PLAIN_FENCES.has(lang.toLowerCase())) return false;
  return (body.match(BOX)?.length ?? 0) >= 6;
}

export function codeBlockClick(el: HTMLElement): boolean {
  const copyBtn = el.closest<HTMLElement>("[data-copy]");
  if (copyBtn) {
    const code = copyBtn.closest(".codeblock")?.querySelector("code")?.textContent ?? "";
    navigator.clipboard.writeText(code).then(
      () => {
        copyBtn.textContent = "Copied";
        setTimeout(() => (copyBtn.textContent = "Copy"), 1200);
      },
      (e) => toast(`Couldn't copy: ${e}`, true),
    );
    return true;
  }
  const expand = el.closest<HTMLElement>("[data-expand]");
  if (expand) {
    const block = expand.closest(".codeblock");
    block?.classList.toggle("clamped");
    expand.textContent = block?.classList.contains("clamped") ? (expand.dataset.label ?? "Show all") : "Show less";
    return true;
  }
  return false;
}

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

md.core.ruler.push("parzi-file-links", (state) => {
  for (const block of state.tokens) {
    if (block.type !== "inline" || !block.children) continue;
    const kids = block.children;
    let depth = 0;
    for (let i = 0; i < kids.length; i++) {
      const tok = kids[i];
      if (tok.type === "link_open") {
        depth++;
        continue;
      }
      if (tok.type === "link_close") {
        depth = Math.max(0, depth - 1);
        continue;
      }
      if (tok.type !== "text" || depth > 0) continue;
      const refs = vaultRefs(tok.content);
      if (!refs.length) continue;
      const nodes: typeof kids = [];
      let at = 0;
      for (const r of refs) {
        if (r.index > at) {
          const plain = new state.Token("text", "", 0);
          plain.content = tok.content.slice(at, r.index);
          nodes.push(plain);
        }
        const open = new state.Token("link_open", "a", 1);
        open.attrs = [
          ["href", r.path],
          ["class", "file-ref"],
        ];
        const label = new state.Token("text", "", 0);
        label.content = r.path;
        const close = new state.Token("link_close", "a", -1);
        nodes.push(open, label, close);
        at = r.index + r.length;
      }
      if (at < tok.content.length) {
        const tail = new state.Token("text", "", 0);
        tail.content = tok.content.slice(at);
        nodes.push(tail);
      }
      kids.splice(i, 1, ...nodes);
      i += nodes.length - 1;
    }
  }
});

reportMarkdown(md);

const mdCache = new Map<string, string>();
const MD_CACHE_MAX = 400;

const LOCAL_IMAGE = /^(data:image\/|asset:|https?:\/\/asset\.localhost\/)/i;

DOMPurify.addHook("afterSanitizeAttributes", (node) => {
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

export function mdHtml(src: string, _rev: unknown, cache = true): string {
  return renderMarkdown(src, cache);
}

export { splitSegments, type Segment } from "./segments";

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
