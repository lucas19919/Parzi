import { writable, type Writable } from "svelte/store";
import { bare } from "./suggest";
import type { Tab } from "./tabs";

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

function read<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function persisted<T>(key: string, fallback: T): Writable<T> {
  const store = writable<T>(read(key, fallback));
  store.subscribe((value) => {
    try {
      localStorage.setItem(key, JSON.stringify(value));
    } catch {}
  });
  return store;
}

export const history = persisted<Visit[]>("parzi.history.v1", []);
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

export function removeVisit(url: string) {
  history.update((all) => all.filter((v) => v.url !== url));
}

export function clearHistory() {
  history.set([]);
}

export function completeAddress(typed: string, visits: Visit[], marks: { url: string }[]): string {
  const t = typed.toLowerCase();
  if (t.length < 2 || /\s/.test(t) || /^https?:/.test(t)) return "";
  const scores = new Map<string, number>();
  const add = (url: string, score: number) => {
    const b = bare(url);
    const c = t.includes("/") ? b : b.split("/")[0];
    if (c.length > t.length && c.toLowerCase().startsWith(t)) scores.set(c, (scores.get(c) ?? 0) + score);
  };
  for (const v of visits) add(v.url, v.count + 1);
  for (const m of marks) add(m.url, 3);
  let best = "";
  let top = 0;
  for (const [c, score] of scores) {
    if (score > top || (score === top && c.length < best.length)) {
      best = c;
      top = score;
    }
  }
  return best;
}

export function addPin(url: string, title: string) {
  pins.update((all) => (all.some((p) => p.url === url) ? all : [...all, { url, title }].slice(0, 12)));
}

export function removePin(url: string) {
  pins.update((all) => all.filter((p) => p.url !== url));
}

const TABS_KEY = "parzi.tabs.v1";

export function saveTabs(tabs: Tab[], active: string) {
  try {
    const slim = tabs.map(({ id, kind, title, sessionId, url, cwd }) => ({ id, kind, title, sessionId, url, cwd }));
    localStorage.setItem(TABS_KEY, JSON.stringify({ tabs: slim, active }));
  } catch {}
}

export function loadTabs(): { tabs: Tab[]; active: string } | null {
  const saved = read<{ tabs: Tab[]; active: string } | null>(TABS_KEY, null);
  if (!saved || !Array.isArray(saved.tabs) || !saved.tabs.length) return null;
  const tabs = saved.tabs.filter((t) => t && typeof t.id === "string" && (t.kind === "session" || t.kind === "page" || t.kind === "brain" || t.kind === "history"));
  if (!tabs.length) return null;
  return { tabs, active: tabs.some((t) => t.id === saved.active) ? saved.active : tabs[0].id };
}

export function faviconUrl(url: string): string {
  try {
    const u = new URL(url);
    return /^https?:$/.test(u.protocol) ? `${u.origin}/favicon.ico` : "";
  } catch {
    return "";
  }
}

export interface Bookmark {
  title: string;
  url: string;
  folder: string;
}

export const bookmarks = persisted<Bookmark[]>("parzi.bookmarks.v1", []);

export function removeBookmark(url: string) {
  bookmarks.update((all) => all.filter((b) => b.url !== url));
}

export function importBrowser(data: {
  bookmarks: Bookmark[];
  history: { url: string; title: string; visits: number; last_visit: number }[];
}) {
  const marks = data.bookmarks.filter((b) => /^https?:\/\//i.test(b.url));
  bookmarks.update((all) => {
    const seen = new Set(all.map((b) => b.url));
    return [...all, ...marks.filter((b) => !seen.has(b.url))];
  });
  pins.update((all) => {
    if (all.length) return all;
    const bar = marks.filter((b) => /bar/i.test(b.folder.split("/")[0] ?? ""));
    return (bar.length ? bar : marks).slice(0, 8).map((b) => ({ url: b.url, title: b.title }));
  });
  history.update((all) => {
    const byUrl = new Map(all.map((v) => [v.url, v]));
    for (const h of data.history) {
      if (!/^https?:\/\//i.test(h.url)) continue;
      const prev = byUrl.get(h.url);
      byUrl.set(h.url, {
        url: h.url,
        title: prev?.title || h.title,
        at: Math.max(prev?.at ?? 0, h.last_visit),
        count: Math.max(prev?.count ?? 0, h.visits),
      });
    }
    return [...byUrl.values()].sort((a, b) => b.at - a.at).slice(0, HISTORY_MAX);
  });
  return { bookmarks: marks.length, history: data.history.length };
}

export const onboarded = persisted<boolean>("parzi.onboarded.v1", false);
