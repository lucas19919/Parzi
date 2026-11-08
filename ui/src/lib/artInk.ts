/** How bright the wallpaper is behind a piece of text, so text that sits
 *  straight on the picture can take whichever ink reads there. */

/** One sample per this many CSS pixels: plenty for text-sized boxes. */
export const STEP = 6;

/** Relative luminance (WCAG) of one 0..255 sRGB channel, linearized. */
const LINEAR = Array.from({ length: 256 }, (_, i) => {
  const c = i / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
});

export function luminance(r: number, g: number, b: number): number {
  return 0.2126 * LINEAR[r] + 0.7152 * LINEAR[g] + 0.0722 * LINEAR[b];
}

/** Behind dark text (about #1c1c1c) or light text (about #f7f6f3), the two
 *  have equal contrast at a luminance near 0.2: darker than that, light ink
 *  reads better. */
export const SWITCH = 0.2;

export function inkFor(lum: number): "dark" | "light" {
  return lum < SWITCH ? "dark" : "light";
}

/** Where an image lands inside a box with object-fit: cover and the given
 *  object-position (0..1 on each axis). */
export function coverRect(iw: number, ih: number, cw: number, ch: number, px = 0.5, py = 0.5) {
  const scale = Math.max(cw / iw, ch / ih);
  const dw = iw * scale;
  const dh = ih * scale;
  // + 0 turns a -0 (no overflow on that axis) into a plain 0.
  return { dx: (cw - dw) * px + 0, dy: (ch - dh) * py + 0, dw, dh };
}

/** "50% 30%" (or keywords) as fractions; anything else is centred. */
export function parsePosition(pos: string): [number, number] {
  const words: Record<string, number> = { left: 0, top: 0, center: 0.5, right: 1, bottom: 1 };
  const part = (s: string | undefined) => {
    if (!s) return 0.5;
    if (s in words) return words[s];
    const m = /^(-?[\d.]+)%$/.exec(s);
    return m ? Number(m[1]) / 100 : 0.5;
  };
  const [x, y] = pos.trim().split(/\s+/);
  return [part(x), part(y)];
}

export type Cells = { lum: Float32Array; w: number; h: number; step: number };

/** Turns RGBA pixels (one per cell) into a grid of luminances. */
export function toCells(rgba: ArrayLike<number>, w: number, h: number, step = STEP): Cells {
  const lum = new Float32Array(w * h);
  for (let i = 0; i < w * h; i++) lum[i] = luminance(rgba[i * 4], rgba[i * 4 + 1], rgba[i * 4 + 2]);
  return { lum, w, h, step };
}

type Box = { left: number; top: number; width: number; height: number };

/** Mean luminance behind `rect`, both in page pixels, `art` being where the
 *  picture is drawn. Null when the rect is empty or off the picture. */
export function cellLuminance(cells: Cells, rect: Box, art: Box): number | null {
  if (rect.width <= 0 || rect.height <= 0) return null;
  const x0 = Math.max(0, Math.floor((rect.left - art.left) / cells.step));
  const y0 = Math.max(0, Math.floor((rect.top - art.top) / cells.step));
  const x1 = Math.min(cells.w, Math.ceil((rect.left + rect.width - art.left) / cells.step));
  const y1 = Math.min(cells.h, Math.ceil((rect.top + rect.height - art.top) / cells.step));
  if (x1 <= x0 || y1 <= y0) return null;
  let sum = 0;
  for (let y = y0; y < y1; y++) for (let x = x0; x < x1; x++) sum += cells.lum[y * cells.w + x];
  return sum / ((x1 - x0) * (y1 - y0));
}

/** Draws `source` the way `shown` shows it inside `art`, one pixel per cell,
 *  and reads it back. Null if the page may not read the picture's pixels. */
export function canSample(source: HTMLImageElement, art: HTMLElement, shown: HTMLImageElement): Cells | null {
  const box = art.getBoundingClientRect();
  const w = Math.ceil(box.width / STEP);
  const h = Math.ceil(box.height / STEP);
  if (!w || !h || !source.naturalWidth) return null;
  const c = document.createElement("canvas");
  c.width = w;
  c.height = h;
  const g = c.getContext("2d", { willReadFrequently: true });
  if (!g) return null;
  const [px, py] = parsePosition(getComputedStyle(shown).objectPosition);
  const r = coverRect(source.naturalWidth, source.naturalHeight, box.width, box.height, px, py);
  g.drawImage(source, r.dx / STEP, r.dy / STEP, r.dw / STEP, r.dh / STEP);
  try {
    return toCells(g.getImageData(0, 0, w, h).data, w, h);
  } catch {
    return null;
  }
}
