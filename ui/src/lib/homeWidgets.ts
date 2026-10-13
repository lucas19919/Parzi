import { writable } from "svelte/store";
import { brain } from "./api";
import { persisted } from "./browserData";
import { isCustomId, isWidgetNote, vizBlock, widgetTitle } from "./homeLayout";
import { parseViz } from "./viz/spec";

// The cards under the composer on Home, in the order they show by default.
export const HOME_WIDGETS = [
  { id: "recent", label: "Recent sessions" },
  { id: "agents", label: "Agents" },
  { id: "projects", label: "Projects" },
  { id: "git", label: "Git" },
  { id: "server", label: "Server" },
  { id: "sites", label: "Pinned sites" },
] as const;

export type HomeWidget = (typeof HOME_WIDGETS)[number]["id"];

// A card's id: a built-in's name, or a custom widget's note path.
export type WidgetId = HomeWidget | string;

// Which cards are hidden, and the order they go in: per-device
// preferences. The order may be empty (the default). Custom ids stay in
// both even while their note can't be read (the vault may not be ready),
// so a widget keeps its place and its hidden state.
export const hiddenWidgets = persisted<WidgetId[]>("parzi.home.hidden.v1", []);
export const widgetOrder = persisted<WidgetId[]>("parzi.home.order.v1", []);

// Built-in cards that are gone (Tasks, then Work) leave both lists.
const known = (all: WidgetId[]) => {
  const kept = Array.isArray(all) ? all.filter((id) => typeof id === "string" && (isCustomId(id) || HOME_WIDGETS.some((w) => w.id === id))) : [];
  return kept.length === all?.length ? all : kept;
};
hiddenWidgets.update(known);
widgetOrder.update(known);

export function toggleWidget(id: WidgetId) {
  hiddenWidgets.update((all) => (all.includes(id) ? all.filter((x) => x !== id) : [...all, id]));
}

// Custom widgets: Brain notes under widgets/ whose body holds a ```viz
// block. Agents write them; null until read the first time.
export interface CustomWidget {
  id: string;
  title: string;
  // The viz block's JSON text (null: the note has none).
  spec: string | null;
}

export const customWidgets = writable<CustomWidget[] | null>(null);

let loading: Promise<void> | null = null;
// Asked again while reading: one more read when this one ends, so a note
// written meanwhile isn't missed.
let again = false;

export function loadCustomWidgets(): Promise<void> {
  if (loading) {
    again = true;
    return loading;
  }
  loading = (async () => {
    do {
      again = false;
      await readCustomWidgets();
    } while (again);
    loading = null;
  })();
  return loading;
}

async function readCustomWidgets() {
  try {
    const notes = (await brain.list()).filter((n) => isWidgetNote(n.path) && !n.conflict_of);
    const texts = await Promise.all(notes.map((n) => brain.read(n.path).catch(() => "")));
    const list = notes
      .map((n, i) => {
        const spec = vizBlock(texts[i]);
        const parsed = spec ? parseViz(spec) : null;
        return { id: n.path, title: widgetTitle(n.title, n.path, parsed?.ok ? parsed.spec.title : ""), spec };
      })
      .sort((a, b) => a.id.localeCompare(b.id));
    customWidgets.set(list);
  } catch {
    // The vault can't be read right now: keep the cards Home has.
  }
}
