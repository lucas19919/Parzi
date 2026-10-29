import { derived, writable } from "svelte/store";

const open = writable(new Set<string>());

export const covered = derived(open, (s) => s.size > 0);

export function setOverlay(key: string, on: boolean) {
  open.update((s) => {
    if (on === s.has(key)) return s;
    const next = new Set(s);
    if (on) next.add(key);
    else next.delete(key);
    return next;
  });
}

export interface PageArea {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

let area: PageArea | null = null;

export function setPageArea(next: PageArea | null) {
  area = next;
}

export function pageArea(): PageArea | null {
  return area;
}
