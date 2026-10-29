import { writable, get } from "svelte/store";
import { api, onboard, onProviders, onSignIn, type ProviderStatus } from "./api";
import { nameOf } from "./providerRows";
import { toastError } from "./toast";

export const board = writable<ProviderStatus[]>([]);
const boardLoaded = writable(false);
export const checking = writable<Set<string>>(new Set());

export interface SetupWait {
  action: "install" | "sign_in";
  note: string;
  eta: number;
}
export const setupWaits = writable<Map<string, SetupWait>>(new Map());

const WATCH_EVERY = 5_000;
const INSTALL_WAIT = 600_000;
const SIGN_IN_WAIT = 180_000;
const START_ETA = 75_000;
const STALE_SECS = 120;

let loading: Promise<ProviderStatus[]> | null = null;
let listening = false;

function listen() {
  if (listening) return;
  listening = true;
  void onProviders((fresh) => merge(fresh));
  void onSignIn(({ provider, step }) => {
    const wait = get(setupWaits).get(provider);
    if (!wait) return;
    setWait(
      provider,
      step === "starting"
        ? { ...wait, note: `Starting ${nameOf(provider)}. This takes about a minute…`, eta: START_ETA }
        : { ...wait, note: "Finish signing in in your browser…", eta: 0 },
    );
  });
}

function merge(fresh: ProviderStatus[]) {
  if (!Array.isArray(fresh)) return;
  board.set(fresh);
  boardLoaded.set(true);
}

function setWait(id: string, wait: SetupWait | null) {
  setupWaits.update((m) => {
    const next = new Map(m);
    if (wait) next.set(id, wait);
    else next.delete(id);
    return next;
  });
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

export async function refreshStale(maxAge = STALE_SECS): Promise<void> {
  const all = await ensureBoard();
  const stale = all.filter((p) => p.state !== "disabled" && ageOf(p) >= maxAge).map((p) => p.provider);
  if (stale.length === all.length) await refreshBoard();
  else if (stale.length) await refreshBoard(stale);
}

const stateOf = (id: string) => get(board).find((b) => b.provider === id)?.state;

async function watch(id: string, from: string | undefined, ms: number) {
  const until = Date.now() + ms;
  while (Date.now() < until) {
    await new Promise((r) => setTimeout(r, WATCH_EVERY));
    const fresh = await refreshBoard([id]).catch(() => [] as ProviderStatus[]);
    const now = fresh.find((b) => b.provider === id)?.state;
    if (now && now !== from) return;
  }
}

async function act(id: string, wait: SetupWait, work: () => Promise<number>) {
  listen();
  if (get(setupWaits).has(id)) return;
  const from = stateOf(id);
  setWait(id, wait);
  try {
    const ms = await work();
    if (ms) await watch(id, from, ms);
    else await refreshBoard([id]);
  } catch (e) {
    toastError(e);
  } finally {
    setWait(id, null);
  }
}

export const installAgent = (id: string) =>
  act(id, { action: "install", note: "Installing in the terminal window…", eta: 0 }, async () => {
    await onboard.install(id);
    return INSTALL_WAIT;
  });

export const signInAgent = (id: string) =>
  act(id, { action: "sign_in", note: "Finish signing in in the window that opens…", eta: 0 }, async () =>
    (await onboard.login(id)) ? 0 : SIGN_IN_WAIT,
  );

export function nextStep(p: ProviderStatus | undefined): { label: string; run: (id: string) => Promise<void> } | null {
  switch (p?.state) {
    case "not_installed":
      return { label: "Install", run: installAgent };
    case "signed_out":
      return { label: "Sign in", run: signInAgent };
    case "error":
      return { label: "Try again", run: (id) => refreshBoard([id]).then(() => {}) };
    default:
      return null;
  }
}
