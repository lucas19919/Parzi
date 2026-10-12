import { derived, writable } from "svelte/store";

const open = writable(new Set<string>());

// Anything open here hides the native page (it is drawn above all HTML);
// PageView shows a snapshot of it meanwhile.
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

// Where the native page is drawn right now (null while none is shown), so
// small floating things like tooltips can stay out of it.
let area: PageArea | null = null;

export function setPageArea(next: PageArea | null) {
  area = next;
}

export function pageArea(): PageArea | null {
  return area;
}
