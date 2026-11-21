import { writable, type Writable } from "svelte/store";
import { iconFor, rememberIcon, type Closed } from "./pageRules";
import { restoreTab, type Tab } from "./tabs";
import { toast } from "./toast";

export interface Visit {
  url: string;
  title: string;
  at: number;
  count: number;
}

export interface Pin {
  url: string;
  title: string;
}

const HISTORY_MAX = 2000;

function sameShape(v: unknown, fallback: unknown): boolean {
  if (fallback === null) return true;
  if (Array.isArray(fallback)) return Array.isArray(v);
  if (typeof fallback === "object") return !!v && typeof v === "object" && !Array.isArray(v);
  return typeof v === typeof fallback;
}

function read<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    if (!raw) return fallback;
    const v = JSON.parse(raw) as unknown;
    return sameShape(v, fallback) ? (v as T) : fallback;
  } catch {
    return fallback;
  }
}

let warnedFull = false;

function store(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch (e) {
    if (warnedFull) return;
    warnedFull = true;
    toast(`Couldn't save browser data (storage full?): ${e}`, true);
  }
}

export function persisted<T>(key: string, fallback: T): Writable<T> {
  const s = writable<T>(read(key, fallback));
  s.subscribe((value) => store(key, value));
  return s;
}

export const history = persisted<Visit[]>("parzi.history.v1", []);
/** Sites pinned in earlier versions: still offered as address suggestions. */
export const pins = persisted<Pin[]>("parzi.pins.v1", []);

export function recordVisit(url: string, title = "") {
  if (!/^https?:\/\//i.test(url)) return;
  history.update((all) => {
    const prev = all.find((v) => v.url === url);
    const rest = all.filter((v) => v.url !== url);
    const visit = { url, title: title || prev?.title || "", at: Date.now(), count: (prev?.count ?? 0) + 1 };
    return [visit, ...rest].slice(0, HISTORY_MAX);
  });
}

export function titleVisit(url: string, title: string) {
  history.update((all) => all.map((v) => (v.url === url ? { ...v, title } : v)));
}

const TABS_KEY = "parzi.tabs.v1";

export function saveTabs(tabs: Tab[], active: string) {
  const slim = tabs.map(({ id, kind, title, sessionId, url, cwd, group }) => ({ id, kind, title, sessionId, url, cwd, group }));
  store(TABS_KEY, { tabs: slim, active });
}

export function loadTabs(): { tabs: Tab[]; active: string } | null {
  const saved = read<{ tabs: Tab[]; active: string } | null>(TABS_KEY, null);
  if (!saved || !Array.isArray(saved.tabs) || !saved.tabs.length) return null;
  const tabs = saved.tabs.flatMap((t) => (t && typeof t.id === "string" ? (restoreTab(t) ?? []) : []));
  if (!tabs.length) return null;
  return { tabs, active: tabs.some((t) => t.id === saved.active) ? saved.active : tabs[0].id };
}

export const favicons = persisted<Record<string, string>>("parzi.favicons.v1", {});
let iconMap: Record<string, string> = {};
favicons.subscribe((m) => (iconMap = m));

export function recordFavicon(pageUrl: string, icon: string) {
  const next = rememberIcon(iconMap, pageUrl, icon);
  if (next) favicons.set(next);
}

export function faviconUrl(url: string): string {
  return iconFor(iconMap, url);
}

export const closedTabs = persisted<Closed[]>("parzi.closed.v1", []);

export interface Bookmark {
  title: string;
  url: string;
  folder: string;
}

export const bookmarks = persisted<Bookmark[]>("parzi.bookmarks.v1", []);

export const onboarded = persisted<boolean>("parzi.onboarded.v1", false);
