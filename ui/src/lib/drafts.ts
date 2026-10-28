// What you were typing, kept per session (a new session's draft by its
// tab) across switching sessions and restarting the app. Sending clears it.

import type { PlanQuote } from "./api";

export interface Draft {
  input: string;
  quotes: PlanQuote[];
  attachments: string[];
  /** When it was last typed (ms), so the oldest go first past MAX. */
  at: number;
}

export type Drafts = Record<string, Draft>;

const KEY = "parzi.drafts.v1";
const MAX = 60;

/** A session's drafts by its id; a new session's by its tab. */
export function draftKey(tab: { id: string; sessionId?: string | null }): string {
  return tab.sessionId ? `s:${tab.sessionId}` : `t:${tab.id}`;
}

const empty = (d: Pick<Draft, "input" | "quotes" | "attachments">) => !d.input.trim() && !d.quotes.length && !d.attachments.length;

/** `all` with `key` set to `d` (or dropped when empty), the newest MAX kept. */
export function withDraft(all: Drafts, key: string, d: Pick<Draft, "input" | "quotes" | "attachments">, now = Date.now()): Drafts {
  const next = { ...all };
  if (empty(d)) delete next[key];
  else next[key] = { input: d.input, quotes: [...d.quotes], attachments: [...d.attachments], at: now };
  const keys = Object.keys(next);
  if (keys.length > MAX) {
    for (const k of keys.sort((a, b) => next[a].at - next[b].at).slice(0, keys.length - MAX)) delete next[k];
  }
  return next;
}

/** Whether two drafts say the same (so typing nothing writes nothing). */
export function sameDraft(a: Draft | undefined, b: Pick<Draft, "input" | "quotes" | "attachments">): boolean {
  if (!a) return empty(b);
  return (
    a.input === b.input &&
    a.attachments.length === b.attachments.length &&
    a.attachments.every((x, i) => x === b.attachments[i]) &&
    a.quotes.length === b.quotes.length &&
    a.quotes.every((q, i) => q.id === b.quotes[i]?.id && q.project === b.quotes[i]?.project)
  );
}

export function loadDrafts(): Drafts {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    if (!v || typeof v !== "object" || Array.isArray(v)) return {};
    const out: Drafts = {};
    for (const [k, d] of Object.entries(v as Record<string, Partial<Draft>>)) {
      if (!d || typeof d.input !== "string") continue;
      out[k] = {
        input: d.input,
        quotes: Array.isArray(d.quotes) ? d.quotes : [],
        attachments: Array.isArray(d.attachments) ? d.attachments.filter((a) => typeof a === "string") : [],
        at: typeof d.at === "number" ? d.at : 0,
      };
    }
    return out;
  } catch {
    return {};
  }
}

let pending = 0;
let latest: Drafts | null = null;

/** Written a moment after the last keystroke, and right away on close. */
export function saveDrafts(all: Drafts): void {
  latest = all;
  if (pending) return;
  pending = window.setTimeout(flushDrafts, 400);
}

export function flushDrafts(): void {
  if (pending) window.clearTimeout(pending);
  pending = 0;
  if (!latest) return;
  try {
    localStorage.setItem(KEY, JSON.stringify(latest));
  } catch {
    // Storage full or blocked: the draft lives on for this run only.
  }
  latest = null;
}
