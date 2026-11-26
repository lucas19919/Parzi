import type { ChatEvent } from "./api";
import type { Step } from "./live";

/** What the thread shows of the saved events. */
export type Item =
  | { key: string; kind: "user" | "assistant"; text: string; at?: number }
  /** An agent's echo of a tool call: part of the work, not the reply. */
  | { key: string; kind: "echo"; text: string; at?: number }
  /** The turn was stopped; only the turn's line says so. */
  | { key: string; kind: "stop"; at?: number }
  | { key: string; kind: "error"; class: string; message: string; at?: number }
  | { key: string; kind: "widget"; fence: string; payload: unknown }
  | { key: string; kind: "artifact"; id: string; title: string; artifact_kind: string; version: number; payload: unknown; at?: number }
  | { key: string; kind: "steps"; reasoning: string; tools: Step[] };

/** A turn's work, said in one line. */
export interface Fold {
  key: string;
  steps: number;
  /** When the turn began, in seconds, or 0. */
  start: number;
  secs: number;
  live: boolean;
  /** The one step, when that is all the turn did. */
  only: string;
  stopped: boolean;
  /** Saved work to show when the line opens. */
  work: boolean;
}

export interface Folds {
  /** The line that goes before an item: the first of its turn. */
  before: Map<string, Fold>;
  /** Work items, by the line that holds them. */
  inside: Map<string, string>;
  /** The line of a turn with nothing saved yet, after every item. */
  tail: Fold | null;
  live: Fold | null;
}

/** One line per turn for its work (steps, narration, echoes). `live` is
 *  the running turn's own steps, while one runs. */
export function foldTurns(items: Item[], live: { steps: number } | null): Folds {
  const out: Folds = { before: new Map(), inside: new Map(), tail: null, live: null };
  const starts = [-1, ...items.flatMap((it, i) => (it.kind === "user" ? [i] : []))];
  starts.forEach((u, n) => {
    const end = n + 1 < starts.length ? starts[n + 1] : items.length;
    const range = items.slice(u + 1, end);
    const current = !!live && n === starts.length - 1;
    const answer = range.map((it) => it.kind).lastIndexOf("assistant");
    const work = range.filter((it, i) => {
      if (it.kind === "steps" || it.kind === "echo") return true;
      if (it.kind !== "assistant" || i === answer) return false;
      // Words that lead into an artifact stay with it.
      let j = i + 1;
      while (j < range.length && range[j].kind === "steps") j++;
      return range[j]?.kind !== "artifact";
    });
    const stopped = range.some((it) => it.kind === "stop");
    if (!work.length && !current && !stopped) return;
    const opener = u >= 0 ? items[u] : null;
    const times = [opener, ...range].map((it) => (it && "at" in it ? it.at ?? 0 : 0)).filter((t) => t > 0);
    const tools = work.flatMap((it) => (it.kind === "steps" ? it.tools : []));
    const fold: Fold = {
      key: `fold-${opener?.key ?? "start"}`,
      steps: tools.length + (current && live ? live.steps : 0),
      start: times.length ? Math.min(...times) : 0,
      secs: times.length > 1 ? Math.max(...times) - Math.min(...times) : 0,
      live: current,
      only: !current && work.length === 1 && tools.length === 1 ? tools[0].label.trim() : "",
      stopped,
      work: work.length > 0,
    };
    if (current) out.live = fold;
    for (const it of work) out.inside.set(it.key, fold.key);
    if (range.length) out.before.set(range[0].key, fold);
    else out.tail = fold;
  });
  return out;
}

/** "42s", "2m", "1h 5m"; `fine` keeps the seconds past a minute. */
export function spent(secs: number, fine = false): string {
  const s = Math.max(0, Math.round(secs));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return fine ? `${m}m ${s % 60}s` : `${m}m`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

/** The turn's line: "Worked 2m · 12 steps", or "Running tests… 10s" / "Thinking… 10s" while it runs. */
export function turnLabel(f: Fold, liveSecs = 0, liveAction = ""): string {
  if (f.live) return `${liveAction || "Thinking…"} ${spent(liveSecs, true)}`;
  if (f.only && !f.stopped) return f.secs >= 1 ? `${f.only} · ${spent(f.secs)}` : f.only;
  const took = f.secs >= 1 ? spent(f.secs) : "";
  const head = f.stopped ? (took ? `Stopped after ${took}` : "Stopped") : took ? `Worked ${took}` : "Worked";
  return f.steps ? `${head} · ${f.steps} step${f.steps === 1 ? "" : "s"}` : head;
}

/** A notice from the transcript, for the Running card. */
export interface Notice {
  key: string;
  /** Milliseconds, or 0 when the event has no time. */
  at: number;
  text: string;
  tip: string;
  /** The session a message came from, or "". */
  from: string;
}

const INTER = "parzi-inter:1";

export function compactedTitle(before?: number, after?: number): string {
  const k = (n: number) => (n >= 1000 ? `${Math.round(n / 1000)}k` : `${n}`);
  if (before && after) return `Conversation compacted · ${k(before)} → ${k(after)} tokens`;
  if (before) return `Conversation compacted · was ${k(before)} tokens`;
  return "Conversation compacted";
}

const clip = (text: string, n: number) => (text.length > n ? `${text.slice(0, n).trimEnd()}…` : text);
const firstLine = (text: string) => clip(text.trim().split("\n")[0].trim(), 140);

/** A message another session sent this one, or null. */
export function interOf(text: string): { from: string; body: string } | null {
  if (!text.startsWith(INTER)) return null;
  try {
    const m = JSON.parse(text.slice(INTER.length));
    return { from: String(m.from_run ?? ""), body: String(m.body ?? "") };
  } catch {
    return null;
  }
}

/** The transcript's system notices, compactions and messages from other
 *  sessions, oldest first. */
export function noticesOf(events: ChatEvent[], title: (session: string) => string): Notice[] {
  const out: Notice[] = [];
  events.forEach((e, i) => {
    const at = (e.at ?? 0) * 1000;
    const key = `n${i}`;
    if (e.kind === "checkpoint") {
      out.push({ key, at, text: compactedTitle(e.before, e.after), tip: clip(e.summary.trim(), 600), from: "" });
    } else if (e.kind === "system" && e.text.trim()) {
      const m = interOf(e.text);
      if (m) out.push({ key, at, text: `${title(m.from)}: ${firstLine(m.body)}`, tip: clip(m.body.trim(), 600), from: m.from });
      else out.push({ key, at, text: firstLine(e.text), tip: clip(e.text.trim(), 600), from: "" });
    }
  });
  return out;
}
