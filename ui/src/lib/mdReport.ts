// Markdown that reads like a report: tables with numbers set right, a
// header's unit under its name, muted n.m. cells and summary rows, a table's
// note, and numbered links as source chips. A markdown-it plugin.
import type MarkdownIt from "markdown-it";

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
}

// Tables read like a report: numbers right-aligned in even figures, a
// header's "(unit)" on a quiet second line, n.m. and dashes muted, Total /
// Mean / Median rows set apart, and an italic line right under the table
// kept as its note.
const NUMBER = /^[(+\-−~≈]?[$€£¥]?\s?\d[\d,]*(?:\.\d+)?\s?(?:%|[kKmMbBxX]|bps|pp|ms|s|h)?\)?$/;
const NOT_APPLICABLE = /^(?:n\.?m\.?|n\/a|na|—|–|-|tbd|\?)$/i;
const SUMMARY_ROW = /^(?:total|sum|mean|median|average|avg)\b/i;
type Tok = ReturnType<MarkdownIt["parse"]>[number];

function cellText(inline: Tok | undefined): string {
  return (inline?.content ?? "").replace(/\*\*|__|`/g, "").trim();
}

export function reportMarkdown(md: MarkdownIt): void {
  md.core.ruler.push("parzi-tables", (state) => {
    const toks = state.tokens;
    for (let i = 0; i < toks.length; i++) {
      if (toks[i].type !== "table_open") continue;
      let head: { cells: { open: Tok; inline: Tok; close: Tok }[] } | null = null;
      const rows: {
        tr: Tok;
        cells: { open: Tok; inline: Tok; close: Tok }[];
      }[] = [];
      let inHead = false;
      let row: (typeof rows)[number] | null = null;
      let j = i + 1;
      for (; j < toks.length && toks[j].type !== "table_close"; j++) {
        const t = toks[j];
        if (t.type === "thead_open") inHead = true;
        else if (t.type === "thead_close") inHead = false;
        else if (t.type === "tr_open") {
          row = { tr: t, cells: [] };
          if (inHead) head = row;
          else rows.push(row);
        } else if ((t.type === "th_open" || t.type === "td_open") && row) {
          row.cells.push({ open: t, inline: toks[j + 1], close: toks[j + 2] });
        }
      }
      const cols = head?.cells.length ?? 0;
      for (let c = 0; c < cols; c++) {
        const vals = rows.map((r) => cellText(r.cells[c]?.inline)).filter((v) => v && !NOT_APPLICABLE.test(v));
        if (vals.length && vals.every((v) => NUMBER.test(v))) {
          head?.cells[c].open.attrJoin("class", "num");
          for (const r of rows) r.cells[c]?.open.attrJoin("class", "num");
        }
      }
      for (const r of rows) {
        for (const cell of r.cells) if (NOT_APPLICABLE.test(cellText(cell.inline))) cell.open.attrJoin("class", "na");
        if (SUMMARY_ROW.test(cellText(r.cells[0]?.inline))) r.tr.attrJoin("class", "summary");
      }
      // "Growth (FY25A)": the unit goes under the name.
      for (const cell of head?.cells ?? []) {
        const kids = cell.inline.children ?? [];
        const last = kids[kids.length - 1];
        const m = last?.type === "text" ? /^(.*\S)\s*\(([^()]{1,24})\)$/.exec(last.content) : null;
        if (!m) continue;
        last.content = m[1];
        cell.close.meta = { sub: m[2] };
      }
      // An italic line straight under the table is its note.
      const after = toks[j + 1];
      const kids = toks[j + 2]?.children ?? [];
      if (after?.type === "paragraph_open" && kids[0]?.type === "em_open" && kids[kids.length - 1]?.type === "em_close") {
        after.attrJoin("class", "md-note");
      }
      i = j;
    }
  });

  // A link that is only a number ("[2](https://…)") is a source: a small
  // chip that names its site on hover.
  md.core.ruler.push("parzi-cites", (state) => {
    for (const tok of state.tokens) {
      const kids = tok.children ?? [];
      for (let k = 0; k + 2 < kids.length; k++) {
        if (kids[k].type !== "link_open" || kids[k + 2].type !== "link_close") continue;
        if (kids[k + 1].type !== "text" || !/^\d{1,3}$/.test(kids[k + 1].content.trim())) continue;
        kids[k].attrJoin("class", "cite");
        try {
          kids[k].attrSet("title", new URL(kids[k].attrGet("href") ?? "").hostname.replace(/^www\./, ""));
        } catch {
          // Not a web address: no site to name.
        }
      }
    }
  });

  md.renderer.rules.table_open = () => '<div class="md-table"><table>\n';
  md.renderer.rules.table_close = () => "</table></div>\n";
  md.renderer.rules.th_close = (tokens, idx) => {
    const sub = tokens[idx].meta?.sub as string | undefined;
    return sub ? `<span class="th-sub">${escapeHtml(sub)}</span></th>` : "</th>";
  };
}
