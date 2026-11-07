/**
 * The loading veil over a page tab. Typing a new address makes the
 * navigation fresh, and while it is fresh and loading the page hides behind
 * a placeholder so a blank webview never flashes in. A finished load ends
 * it, and so does the caller's failsafe timer.
 */
export interface Veil {
  url: string;
  loading: boolean;
  fresh: boolean;
}

export const NO_VEIL: Veil = { url: "", loading: false, fresh: false };

export interface VeilStep {
  veil: Veil;
  /** The address changed while no load was finishing: a fresh navigation began. */
  started: boolean;
  /** A load finished: loading went from true to false. */
  finished: boolean;
}

/**
 * Advance the veil to the tab's current address and loading flag. Calling it
 * again with the same values changes nothing, so it can run on every tab
 * update. A finished load wins over an address change in the same update:
 * pages report their final address together with the end of the load.
 */
export function stepVeil(prev: Veil, url: string, loading: boolean): VeilStep {
  const finished = prev.loading && !loading;
  const started = !!url && url !== prev.url && !finished;
  const fresh = finished ? false : started ? true : prev.fresh;
  return { veil: { url, loading, fresh }, started, finished };
}
