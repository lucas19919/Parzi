import MarkdownIt from "markdown-it";
import DOMPurify from "dompurify";
import hljs from "highlight.js";

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
    const htmlLines = html.replace(/\n$/, "").split("\n");
    code = htmlLines
      .map((l, i) => `<span class="cl-line"><span class="cl-no">${i + 1}</span><span class="cl-tx">${l || " "}</span></span>`)
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
/** GFM-ish task lists without a plugin: `- [ ]` / `- [x]` rendering. */
md.core.ruler.push("parzi-tasklists", (state) => {
  for (const tok of state.tokens) {
    if (tok.type === "inline" && tok.content.startsWith("[ ] ")) {
      tok.content = "☐ " + tok.content.slice(4);
    } else if (tok.type === "inline" && tok.content.startsWith("[x] ")) {
      tok.content = "☑ " + tok.content.slice(4);
    }
  }
});

export function renderMarkdown(src: string): string {
  return DOMPurify.sanitize(md.render(src), {
    ALLOWED_URI_REGEXP: /^(?:(?:https?|parzi):|[^a-z]|[a-z+.-]+(?:[^a-z+.\-:]|$))/i,
  });
}

export type Segment =
  | { kind: "md"; body: string }
  | { kind: "widget"; body: unknown }
  | { kind: "diagram"; body: unknown }
  | { kind: "artifact"; body: any };

/** Split ```parzi-widget / ```parzi-diagram / ```parzi-artifact fences out for component rendering. */
export function splitSegments(text: string): Segment[] {
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
  return out;
}
