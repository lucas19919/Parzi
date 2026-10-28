// Where a `$…$` (or one-line `$$…$$`) math span starting at `pos` ends, or
// null when the dollar sign is money. Pandoc's rule: an opening `$` has a
// non-space right after it, a closing `$` a non-space right before it and
// no digit right after it, so "$40 billion, or $24 billion" stays text.

export interface MathSpan {
  display: boolean;
  /** The TeX between the delimiters. */
  tex: string;
  /** Where the text after the span starts. */
  next: number;
}

export function mathAt(src: string, pos: number): MathSpan | null {
  if (src[pos] !== "$" || (pos > 0 && src[pos - 1] === "\\")) return null;
  const display = src[pos + 1] === "$";
  const open = display ? pos + 2 : pos + 1;
  if (!display && /\s/.test(src[open] ?? " ")) return null;
  for (let end = open; end < src.length; end++) {
    const c = src[end];
    if (c === "\n") return null;
    if (c !== "$" || src[end - 1] === "\\") continue;
    if (display) {
      if (src[end + 1] !== "$") continue;
    } else if (src[end + 1] === "$" || /\s/.test(src[end - 1]) || /\d/.test(src[end + 1] ?? "")) {
      continue;
    }
    const tex = src.slice(open, end);
    if (!tex.trim()) return null;
    return { display, tex, next: display ? end + 2 : end + 1 };
  }
  return null;
}
