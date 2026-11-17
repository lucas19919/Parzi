export interface MathSpan {
  display: boolean;
  tex: string;
  next: number;
}

// Inline math draws fractions, sums and integrals in KaTeX's compact text
// style, about two thirds size: De/Dt in a sentence is too small to read. They
// get display size instead; an author's \tfrac (asked small) stays small.
const TALL = /\\(?:frac|dfrac|sum|prod|int|iint|iiint|oint|lim|binom)(?![a-zA-Z])/;

export function readableTex(tex: string, display: boolean): string {
  if (display || !TALL.test(tex) || /\\(?:displaystyle|textstyle|scriptstyle)/.test(tex)) return tex;
  return `\\displaystyle ${tex}`;
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
