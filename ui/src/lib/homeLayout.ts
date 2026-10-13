// Home's cards: the order they show in and the custom ones agents write.
// Pure, so tests pin it down; homeWidgets.ts keeps the stores.

// A custom widget is a Brain note under widgets/ holding a ```viz block;
// its id is the note's path.
export const WIDGET_DIR = "widgets/";

export function isWidgetNote(path: string): boolean {
  return path.startsWith(WIDGET_DIR) && path.toLowerCase().endsWith(".md") && !/\.conflict-[^/]*\.md$/i.test(path);
}

export function isCustomId(id: string): boolean {
  return id.includes("/");
}

// The first ```viz block's JSON in a note, or null without one.
export function vizBlock(text: string): string | null {
  const m = /^[ \t]*(`{3,}|~{3,})[ \t]*viz[ \t]*\r?\n([\s\S]*?)\r?\n[ \t]*\1[ \t]*$/m.exec(text);
  return m ? m[2] : null;
}

// The card's name: the note's own title (frontmatter or heading), else the
// chart's title, else the file name in words.
export function widgetTitle(noteTitle: string, path: string, chartTitle = ""): string {
  const stem = path.replace(/^.*\//, "").replace(/\.md$/i, "");
  if (noteTitle && noteTitle !== stem) return noteTitle;
  if (chartTitle) return chartTitle;
  const words = stem.replace(/[-_]+/g, " ").trim();
  return words ? words[0].toUpperCase() + words.slice(1) : "Widget";
}

// The saved order applied to the cards there are now: saved ones keep
// their place, ones that are gone drop out, new ones join at the end.
export function arrange(saved: string[], ids: string[]): string[] {
  const have = new Set(ids);
  const kept = saved.filter((id, i) => have.has(id) && saved.indexOf(id) === i);
  const placed = new Set(kept);
  return [...kept, ...ids.filter((id) => !placed.has(id))];
}

// `id` moved to `index` (clamped), the rest keeping their order.
export function moveTo(order: string[], id: string, index: number): string[] {
  const from = order.indexOf(id);
  if (from < 0) return order;
  const rest = order.filter((x) => x !== id);
  const at = Math.max(0, Math.min(rest.length, index));
  if (at === from) return order;
  return [...rest.slice(0, at), id, ...rest.slice(at)];
}

// One step up (-1) or down (1).
export function moveBy(order: string[], id: string, step: number): string[] {
  const from = order.indexOf(id);
  return from < 0 ? order : moveTo(order, id, from + step);
}

// The full order with its shown cards put in `shown`'s order; hidden
// cards keep their places. Ids `order` doesn't have (a widget deleted while
// a card was dragged) are left out, so no card is lost.
export function withShown(order: string[], shown: string[]): string[] {
  const have = new Set(order);
  const list = shown.filter((id, i) => have.has(id) && shown.indexOf(id) === i);
  const set = new Set(list);
  let next = 0;
  return order.map((id) => (set.has(id) ? list[next++] : id));
}

// A new order of the cards Home has now, saved over the old one: a saved id
// Home doesn't have yet (a custom widget not read yet) stays right after
// the card it followed instead of falling to the end.
export function mergeOrder(saved: string[], next: string[]): string[] {
  const placed = new Set(next);
  const out = [...next];
  saved.forEach((id, i) => {
    if (placed.has(id)) return;
    let at = 0;
    for (let j = i - 1; j >= 0; j--) {
      const k = out.indexOf(saved[j]);
      if (k >= 0) {
        at = k + 1;
        break;
      }
    }
    out.splice(at, 0, id);
    placed.add(id);
  });
  return out;
}

export interface Box {
  left: number;
  top: number;
  width: number;
  height: number;
}

export function clamp(v: number, lo: number, hi: number): number {
  return Math.max(lo, Math.min(hi, v));
}

// Where a dragged card lands: the slot whose centre is nearest to the
// card's own centre (x, y). Slots don't move while a card is dragged, so
// the answer only changes when the card crosses halfway to another slot.
export function nearestSlot(slots: Box[], x: number, y: number): number {
  let best = -1;
  let dist = Infinity;
  slots.forEach((b, i) => {
    const d = Math.hypot(b.left + b.width / 2 - x, b.top + b.height / 2 - y);
    if (d < dist) {
      dist = d;
      best = i;
    }
  });
  return best;
}

// The rows of a grid of cards, top to bottom: the cards sharing a top.
export function rowsOf(boxes: { top: number; height: number }[]): { top: number; bottom: number }[] {
  const rows = new Map<number, number>();
  for (const b of boxes) rows.set(b.top, Math.max(rows.get(b.top) ?? 0, b.top + b.height));
  return [...rows].sort((a, b) => a[0] - b[0]).map(([top, bottom]) => ({ top, bottom }));
}

// Home's cards scroll with no scrollbar. When they don't all fit, the list
// stops partway down a row, under the bottom fade, so the cut cards say
// there is more: at least PEEK of the next row (its top edge shows through
// the fade), or else the last whole row loses CUT. `rows` are in list px
// (padding included), `end` is where the content ends. Returns the list's
// height, at most `avail`.
export const PEEK = 36;
export const CUT = 28;

export function peekHeight(avail: number, rows: { top: number; bottom: number }[], end: number): number {
  if (end <= avail + 1) return avail;
  const at = rows.findIndex((r) => r.bottom > avail);
  if (at < 0) return avail;
  if (avail - rows[at].top >= PEEK) return avail;
  const prev = rows[at - 1];
  if (!prev || prev.bottom - CUT < prev.top + PEEK) return avail;
  return prev.bottom - CUT;
}

// The list's edge fades, px: the top one once scrolled; at the bottom the
// padding's fade at the end, deeper while there is more below.
export const END_FADE = 32;

export function fades(scrollTop: number, scrollHeight: number, clientHeight: number): { top: number; bottom: number } {
  const top = Math.max(0, scrollTop);
  const below = Math.max(0, scrollHeight - clientHeight - top);
  return { top: Math.min(top, 16), bottom: END_FADE + Math.min(below, 24) };
}
