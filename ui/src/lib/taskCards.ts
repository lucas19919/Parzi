import { writable } from "svelte/store";

// What became of each task card an agent proposed: started (as which side
// task) or dismissed. Kept on this computer; a card left alone stays open.
export type CardState = { state: "started"; session: string } | { state: "dismissed" };

const KEY = "parzi.tasks";

function load(): Record<string, CardState> {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    return v && typeof v === "object" ? v : {};
  } catch {
    return {};
  }
}

export const taskCards = writable<Record<string, CardState>>(load());
taskCards.subscribe((v) => {
  try {
    localStorage.setItem(KEY, JSON.stringify(v));
  } catch {
    /* private window: the state holds until a restart */
  }
});

export function markCard(id: string, s: CardState) {
  taskCards.update((all) => ({ ...all, [id]: s }));
}
