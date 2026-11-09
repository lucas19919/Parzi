import { writable } from "svelte/store";
import { readJSON, writeJSON } from "./storage.ts";

export type CardState = { state: "started"; session: string } | { state: "dismissed" };

const KEY = "parzi.tasks";

export const taskCards = writable<Record<string, CardState>>(readJSON(KEY, {}, (v) => !!v && typeof v === "object"));
taskCards.subscribe((v) => writeJSON(KEY, v));

export function markCard(id: string, s: CardState) {
  taskCards.update((all) => ({ ...all, [id]: s }));
}
