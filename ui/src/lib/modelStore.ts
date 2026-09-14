import { writable, get } from "svelte/store";
import { api, type ModelRow } from "./api";

/** Shared model catalog: fetched once at startup, reused by the picker and
 *  Settings > Models. Fast path (cached catalog, no network) by default;
 *  pass `refresh: true` for a live pull. */
export const modelRows = writable<ModelRow[]>([]);
export const modelsLoaded = writable(false);

let inflight: Promise<ModelRow[]> | null = null;

export function ensureModels(refresh = false): Promise<ModelRow[]> {
  if (!refresh && get(modelsLoaded)) return Promise.resolve(get(modelRows));
  if (inflight) return inflight;
  inflight = api
    .getModels(refresh)
    .then((rows) => {
      modelRows.set(rows);
      modelsLoaded.set(true);
      inflight = null;
      return rows;
    })
    .catch((e) => {
      inflight = null;
      throw e;
    });
  return inflight;
}

/** Force a live refresh (network). Used by the manual Refresh buttons. */
export function refreshModels(): Promise<ModelRow[]> {
  modelsLoaded.set(false);
  return ensureModels(true);
}

/** Swap one provider's row after a live pull (model picker step two). */
export function updateProviderRow(row: ModelRow): void {
  modelRows.update((rows) => {
    const i = rows.findIndex((r) => r.provider === row.provider);
    if (i < 0) return [...rows, row];
    const next = rows.slice();
    next[i] = row;
    return next;
  });
}
