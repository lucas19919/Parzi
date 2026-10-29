// Messages typed while a session is already working. Each one stays its own
// turn and goes out in order when the run finishes. They used to be joined
// into a single blob, so a later message could not be sent, edited or
// dropped on its own, and a failed send left the composer looking busy.

import type { PlanQuote } from "./api";

export interface HeldItem {
  id: string;
  tab: string;
  text: string;
  quotes: PlanQuote[];
  files: string[];
  model: string;
  effort: string;
  mode: string;
  cwd: string;
  remote: boolean;
}

export type HoldQueue = Record<string, HeldItem[]>;

/** Enough follow-ups for one run. Past this the composer keeps the draft. */
export const MAX_HELD = 8;

const KEY = "parzi.held.v1";

export function heldId(): string {
  return `h${Date.now().toString(36)}${Math.floor(Math.random() * 0xffffff).toString(36)}`;
}

export function heldLabel(item: { text: string; quotes: { title: string }[] }): string {
  const text = item.text.trim();
  if (text) return text;
  return item.quotes.map((q) => q.title).filter(Boolean).join(", ");
}

function write(queue: HoldQueue, sid: string, items: HeldItem[]): HoldQueue {
  const next = { ...queue };
  if (items.length) next[sid] = items;
  else delete next[sid];
  return next;
}

export function enqueue(queue: HoldQueue, sid: string, item: Omit<HeldItem, "id"> & { id?: string }, max = MAX_HELD): { queue: HoldQueue; ok: boolean } {
  const list = queue[sid] ?? [];
  if (list.length >= max) return { queue, ok: false };
  const next: HeldItem = {
    id: item.id || heldId(),
    tab: item.tab,
    text: item.text,
    quotes: item.quotes.map((q) => ({ ...q })),
    files: [...item.files],
    model: item.model,
    effort: item.effort,
    mode: item.mode,
    cwd: item.cwd,
    remote: item.remote,
  };
  return { queue: { ...queue, [sid]: [...list, next] }, ok: true };
}

export function takeHead(queue: HoldQueue, sid: string): { queue: HoldQueue; item?: HeldItem } {
  const list = queue[sid];
  if (!list?.length) return { queue };
  const [item, ...rest] = list;
  return { queue: write(queue, sid, rest), item };
}

export function takeId(queue: HoldQueue, sid: string, id: string): { queue: HoldQueue; item?: HeldItem } {
  const list = queue[sid];
  const at = list?.findIndex((item) => item.id === id) ?? -1;
  if (!list || at < 0) return { queue };
  const item = list[at];
  return { queue: write(queue, sid, list.filter((entry) => entry.id !== id)), item };
}

export function takeAll(queue: HoldQueue, sid: string): { queue: HoldQueue; items: HeldItem[] } {
  const items = queue[sid] ?? [];
  if (!items.length) return { queue, items: [] };
  const next = { ...queue };
  delete next[sid];
  return { queue: next, items: [...items] };
}

/** The server refused a second run because this session is still in one. */
export function stillWorking(error: unknown): boolean {
  return /still working/i.test(String(error));
}

/** A send that failed goes back to the front. The ones queued behind it stay behind it. */
export function putFront(queue: HoldQueue, sid: string, item: HeldItem): HoldQueue {
  const list = queue[sid] ?? [];
  if (list.some((entry) => entry.id === item.id)) return queue;
  return { ...queue, [sid]: [item, ...list] };
}

/** Drop one queued message without touching the rest. */
export function dropId(queue: HoldQueue, sid: string, id: string): HoldQueue {
  const list = queue[sid];
  if (!list?.some((item) => item.id === id)) return queue;
  return write(queue, sid, list.filter((entry) => entry.id !== id));
}

/** Move one queued message to position `to` (clamped), keeping the rest. */
export function moveId(queue: HoldQueue, sid: string, id: string, to: number): HoldQueue {
  const list = queue[sid];
  const at = list?.findIndex((item) => item.id === id) ?? -1;
  if (!list || at < 0) return queue;
  const rest = list.filter((entry) => entry.id !== id);
  const clamped = Math.max(0, Math.min(to, rest.length));
  return write(queue, sid, [...rest.slice(0, clamped), list[at], ...rest.slice(clamped)]);
}

/** The server continued this session under another id. The queue follows. */
export function moveHeld(queue: HoldQueue, from: string, to: string): HoldQueue {
  if (!from || from === to || !queue[from]?.length) return queue;
  const items = queue[from];
  const next = { ...queue };
  delete next[from];
  next[to] = [...(next[to] ?? []), ...items];
  return next;
}

function cleanItem(raw: Partial<HeldItem> | null): HeldItem | null {
  if (!raw || typeof raw.id !== "string" || typeof raw.text !== "string") return null;
  const quotes: PlanQuote[] = [];
  if (Array.isArray(raw.quotes)) {
    for (const q of raw.quotes) {
      if (!q || typeof q.id !== "string" || typeof q.project !== "string") continue;
      quotes.push({ id: q.id, project: q.project, title: typeof q.title === "string" ? q.title : q.id });
    }
  }
  return {
    id: raw.id,
    tab: typeof raw.tab === "string" ? raw.tab : "",
    text: raw.text,
    quotes,
    files: Array.isArray(raw.files) ? raw.files.filter((f): f is string => typeof f === "string") : [],
    model: typeof raw.model === "string" ? raw.model : "",
    effort: typeof raw.effort === "string" ? raw.effort : "medium",
    mode: typeof raw.mode === "string" ? raw.mode : "full",
    cwd: typeof raw.cwd === "string" ? raw.cwd : "",
    remote: raw.remote === true,
  };
}

export function loadHeld(): HoldQueue {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    if (!v || typeof v !== "object" || Array.isArray(v)) return {};
    const out: HoldQueue = {};
    for (const [sid, list] of Object.entries(v as Record<string, unknown>)) {
      if (!Array.isArray(list)) continue;
      const items = list.map((item) => cleanItem(item as Partial<HeldItem>)).filter((item): item is HeldItem => !!item);
      if (items.length) out[sid] = items.slice(0, MAX_HELD);
    }
    return out;
  } catch {
    return {};
  }
}

let pending = 0;
let latest: HoldQueue | null = null;

export function saveHeld(queue: HoldQueue): void {
  latest = queue;
  if (pending) return;
  pending = window.setTimeout(flushHeld, 400);
}

export function flushHeld(): void {
  if (pending) window.clearTimeout(pending);
  pending = 0;
  if (!latest) return;
  try {
    localStorage.setItem(KEY, JSON.stringify(latest));
  } catch {
    // Storage full or blocked: the queue lives on for this run only.
  }
  latest = null;
}
