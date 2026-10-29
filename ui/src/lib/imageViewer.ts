import { writable } from "svelte/store";

export type Viewed = { src: string; alt: string };
export const viewed = writable<Viewed | null>(null);

export function viewImage(src: string, alt = "") {
  if (src) viewed.set({ src, alt });
}

export function closeImage() {
  viewed.set(null);
}
