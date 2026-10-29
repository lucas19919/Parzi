import { writable } from "svelte/store";

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
  }
});

export function markCard(id: string, s: CardState) {
  taskCards.update((all) => ({ ...all, [id]: s }));
}
