import { writable, get } from "svelte/store";
import { api, onProviders, type ProviderStatus } from "./api";

export const board = writable<ProviderStatus[]>([]);
export const boardLoaded = writable(false);
export const checking = writable<Set<string>>(new Set());

let loading: Promise<ProviderStatus[]> | null = null;
let listening = false;

function listen() {
  if (listening) return;
  listening = true;
  void onProviders((fresh) => merge(fresh));
}

function merge(fresh: ProviderStatus[]) {
  board.set(fresh);
  boardLoaded.set(true);
}

export function ensureBoard(): Promise<ProviderStatus[]> {
  listen();
  if (get(boardLoaded)) return Promise.resolve(get(board));
  if (loading) return loading;
  loading = api
    .providerStatuses()
    .then((all) => {
      merge(all);
      return all;
    })
    .finally(() => {
      loading = null;
    });
  return loading;
}

export async function refreshBoard(ids: string[] = []): Promise<ProviderStatus[]> {
  listen();
  const keys = ids.length ? ids : ["*"];
  checking.update((s) => new Set([...s, ...keys]));
  try {
    const all = await api.refreshProviders(ids);
    merge(all);
    return all;
  } finally {
    checking.update((s) => {
      const next = new Set(s);
      for (const k of keys) next.delete(k);
      return next;
    });
  }
}

export function ageOf(p: ProviderStatus | undefined): number {
  if (!p) return Infinity;
  return Date.now() / 1000 - p.checked_at;
}
