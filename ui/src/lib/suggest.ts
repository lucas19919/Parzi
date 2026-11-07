import type { Bookmark, Pin, Visit } from "./browserData";
import { addressKind, toAddress } from "./tabs.ts";

export interface Suggestion {
  kind: "search" | "url" | "phrase" | "history" | "bookmark";
  text: string;
  url: string;
  title?: string;
}

export function bare(url: string) {
  return url.replace(/^https?:\/\//i, "").replace(/^www\./i, "").replace(/\/$/, "");
}

export function looksLikeUrl(text: string) {
  const kind = addressKind(text);
  return kind !== "search" && kind !== "empty";
}

export function firstRows(typed: string, completed: string): Suggestion[] {
  const t = typed.trim();
  if (!t) return [];
  const rows: Suggestion[] = [];
  if (completed && completed !== t) rows.push({ kind: "url", text: completed, url: toAddress(completed) });
  if (looksLikeUrl(t)) rows.push({ kind: "url", text: t, url: toAddress(t) });
  else rows.push({ kind: "search", text: t, url: toAddress(t) });
  return rows;
}

export function phraseRows(typed: string, phrases: string[]): Suggestion[] {
  const t = typed.trim().toLowerCase();
  return phrases
    .filter((p) => p.trim() && p.trim().toLowerCase() !== t)
    .slice(0, 4)
    .map((p) => ({ kind: "phrase", text: p, url: toAddress(p) }));
}

export function pageRows(typed: string, visits: Visit[], marks: (Bookmark | Pin)[], limit = 5): Suggestion[] {
  const t = typed.trim().toLowerCase();
  if (t.length < 2) return [];
  const scored = new Map<string, { row: Suggestion; score: number }>();
  const consider = (url: string, title: string, kind: "history" | "bookmark", weight: number, at = 0) => {
    const b = bare(url).toLowerCase();
    const name = title.toLowerCase();
    const hit = b.startsWith(t) ? 3 : b.includes(t) ? 1.5 : name.includes(t) ? 1 : 0;
    if (!hit) return;
    const fresh = at ? Math.max(0, 1 - (Date.now() - at) / (30 * 86_400_000)) : 0;
    const score = hit * (weight + fresh);
    const prev = scored.get(url);
    if (!prev || prev.score < score) scored.set(url, { row: { kind, text: title || bare(url), url, title }, score });
  };
  for (const v of visits) consider(v.url, v.title, "history", Math.log2(v.count + 1) + 1, v.at);
  for (const m of marks) consider(m.url, m.title, "bookmark", 2);
  return [...scored.values()]
    .sort((a, b) => b.score - a.score)
    .slice(0, limit)
    .map((s) => s.row);
}

export function mergeRows(...groups: Suggestion[][]): Suggestion[] {
  const seen = new Set<string>();
  const out: Suggestion[] = [];
  for (const row of groups.flat()) {
    const key = row.kind === "search" || row.kind === "phrase" ? `q:${row.text.toLowerCase()}` : `u:${bare(row.url).toLowerCase()}`;
    if (seen.has(key)) continue;
    seen.add(key);
    out.push(row);
  }
  return out.slice(0, 9);
}

export function boxText(row: Suggestion) {
  return row.kind === "search" || row.kind === "phrase" ? row.text : bare(row.url);
}

/** The rest of an address the person has been to, to complete what they typed in place. */
export function completeAddress(typed: string, visits: { url: string; count: number }[], marks: { url: string }[]): string {
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
