import { writable } from "svelte/store";

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
