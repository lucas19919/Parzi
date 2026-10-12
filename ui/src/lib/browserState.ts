import { writable } from "svelte/store";
import type { Download } from "./api";

// What pages ask the user, shown in the asking tab's info bar: a site
// permission, a link for another app, a pop-up that was blocked.
export interface PagePrompt {
  id: string;
  tab: string;
  kind: "permission" | "external" | "popup";
  origin?: string;
  permission?: string;
  url?: string;
}

export const prompts = writable<PagePrompt[]>([]);

export function addPrompt(p: PagePrompt) {
  prompts.update((all) => {
    // One pop-up notice per tab is enough; the newest wins.
    const rest = p.kind === "popup" ? all.filter((x) => !(x.tab === p.tab && x.kind === "popup")) : all;
    return [...rest.filter((x) => x.id !== p.id), p];
  });
}

export function removePrompt(id: string) {
  prompts.update((all) => all.filter((p) => p.id !== id));
}

export function dropTabPrompts(tab: string) {
  prompts.update((all) => all.filter((p) => p.tab !== tab));
}

// Downloads, newest first; the backend keeps the list across restarts.
export const downloadList = writable<Download[]>([]);

export function upsertDownload(d: Download) {
  downloadList.update((all) => [d, ...all.filter((x) => x.id !== d.id)].sort((a, b) => b.started - a.started));
}
