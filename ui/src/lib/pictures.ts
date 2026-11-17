// Pictures a tool made, raster or a drawing (an svg a diagram connector
// wrote). Only what a tool says it made counts: a path in a JSON `path` or
// `svg` field, or one right after "saved", "wrote", "created" and the like.
// A path that merely appears in a listing, a search or a log is not shown.
const IMAGE = /\.(png|jpe?g|webp|gif|svg)$/i;
const FULL = /(?<![\w.\\/:~-])(?:[A-Za-z]:[\\/]|\/(?![\\/]))[^\s"'`<>|*?]+?\.(?:png|jpe?g|webp|gif|svg)\b/gi;
const REMOTE = /^[\\/]{2}/;
const MADE = /\b(?:saved|wrote|written|created|exported|rendered|screenshot)\b[^\n]{0,40}$/i;

export function picturesIn(outputs: string[]): string[] {
  const out: string[] = [];
  const add = (p: string) => {
    const clean = p.trim();
    if (IMAGE.test(clean) && !REMOTE.test(clean) && !out.includes(clean)) out.push(clean);
  };
  for (const text of outputs) {
    if (!text) continue;
    try {
      const v = JSON.parse(text);
      const path = v && typeof v === "object" ? (v.path ?? v.output?.path ?? v.svg) : null;
      if (typeof path === "string") {
        add(path);
        continue;
      }
    } catch {}
    for (const m of text.matchAll(FULL)) {
      const at = m.index ?? 0;
      if (MADE.test(text.slice(Math.max(0, at - 60), at))) add(m[0]);
    }
  }
  return out.slice(0, 12);
}
