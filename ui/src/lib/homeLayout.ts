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
// cards keep their places.
export function withShown(order: string[], shown: string[]): string[] {
  const set = new Set(shown);
  let next = 0;
  return order.map((id) => (set.has(id) ? shown[next++] : id));
}

// A list (the saved order, the hidden cards) without custom widgets that
// no longer exist. Built-in ids are left alone.
export function dropGone(list: string[], customIds: string[]): string[] {
  const have = new Set(customIds);
  const next = list.filter((id) => !isCustomId(id) || have.has(id));
  return next.length === list.length ? list : next;
}
