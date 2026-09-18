import { writable, get } from "svelte/store";
import { api, onProviders, type ProviderStatus } from "./api";

/** The provider board: where each agent stands, as its own program last
 *  said. Loaded once (instant, from disk); the app asks the programs again
 *  at start and whenever Settings says so, and every fresh board arrives
 *  here through the `parzi://providers` event. */
export const board = writable<ProviderStatus[]>([]);
export const boardLoaded = writable(false);
/** Providers being asked right now ("*" = all). */
export const checking = writable<Set<string>>(new Set());

let loading: Promise<ProviderStatus[]> | null = null;
let listening = false;

function listen() {
  if (listening) return;
  listening = true;
  void onProviders((fresh) => merge(fresh));
}

/** A refresh of some providers answers with the whole board: take it all. */
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

/** Ask the agents' own programs again: all of them, or the ones named. */
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

/** Seconds since this provider was last asked; Infinity when never. */
export function ageOf(p: ProviderStatus | undefined): number {
  if (!p) return Infinity;
  return Date.now() / 1000 - p.checked_at;
}
