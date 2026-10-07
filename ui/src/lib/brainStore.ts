import { writable } from "svelte/store";

export const targetBrainNote = writable<string>("");
export const brainTabRequested = writable<number>(0);

export function openBrainNote(path: string) {
  targetBrainNote.set(path);
  brainTabRequested.update((n) => n + 1);
}
