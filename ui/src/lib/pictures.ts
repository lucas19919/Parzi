// Pictures a tool saved, raster or a drawing (an svg a diagram connector wrote).
const IMAGE = /\.(png|jpe?g|webp|gif|svg)$/i;
const FULL = /(?<![\w.\\/:~-])(?:[A-Za-z]:[\\/]|\/(?![\\/]))[^\s"'`<>|*?]+?\.(?:png|jpe?g|webp|gif|svg)\b/gi;
const REMOTE = /^[\\/]{2}/;

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
    } catch {
    }
    for (const m of text.matchAll(FULL)) add(m[0]);
  }
  return out.slice(0, 12);
}
