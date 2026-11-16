export interface Draft {
  input: string;
  attachments: string[];
  at: number;
}

export type Drafts = Record<string, Draft>;

const KEY = "parzi.drafts.v1";
const MAX = 60;

export function draftKey(tab: { id: string; sessionId?: string | null }): string {
  return tab.sessionId ? `s:${tab.sessionId}` : `t:${tab.id}`;
}

const empty = (d: Pick<Draft, "input" | "attachments">) => !d.input.trim() && !d.attachments.length;

export function withDraft(all: Drafts, key: string, d: Pick<Draft, "input" | "attachments">, now = Date.now()): Drafts {
  const next = { ...all };
  if (empty(d)) delete next[key];
  else next[key] = { input: d.input, attachments: [...d.attachments], at: now };
  const keys = Object.keys(next);
  if (keys.length > MAX) {
    for (const k of keys.sort((a, b) => next[a].at - next[b].at).slice(0, keys.length - MAX)) delete next[k];
  }
  return next;
}

export function sameDraft(a: Draft | undefined, b: Pick<Draft, "input" | "attachments">): boolean {
  if (!a) return empty(b);
  return (
    a.input === b.input &&
    a.attachments.length === b.attachments.length &&
    a.attachments.every((x, i) => x === b.attachments[i])
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
  } catch {}
  latest = null;
}
