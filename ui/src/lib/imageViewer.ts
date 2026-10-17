import { writable } from "svelte/store";

// The image shown big over the app, if any. Anything that shows an image
// (a thread, an artifact, the composer) opens it here.
export type Viewed = { src: string; alt: string };
export const viewed = writable<Viewed | null>(null);

export function viewImage(src: string, alt = "") {
  if (src) viewed.set({ src, alt });
}

export function closeImage() {
  viewed.set(null);
}
