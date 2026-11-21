import { writable } from "svelte/store";
import { onRunEvent, type UiEvent } from "./api";
import { stepLog, type RunLog } from "./runLog";

export interface Activity {
  label: string;
  at: number;
}

export const activity = writable<Map<string, Activity>>(new Map());

let pending = new Map<string, Activity | null>();
let flushTimer: ReturnType<typeof setTimeout> | null = null;

function flush() {
  flushTimer = null;
  const batch = pending;
  pending = new Map();
  activity.update((cur) => {
    const next = new Map(cur);
    for (const [s, a] of batch) {
      if (a) next.set(s, a);
      else next.delete(s);
    }
    return next;
  });
}

function stage(session: string, a: Activity | null) {
  pending.set(session, a);
  flushTimer ??= setTimeout(flush, 150);
}

/** Each session's latest run, step by step, for the Running card. */
export const runLogs = writable<Map<string, RunLog>>(new Map());

function trackLog(e: UiEvent) {
  if (!("session" in e) || !e.session) return;
  runLogs.update((all) => {
    const before = all.get(e.session);
    const after = stepLog(before, e);
    if (after === before || !after) return all;
    const next = new Map(all);
    next.set(e.session, after);
    return next;
  });
}

export function trackActivity(e: UiEvent) {
  if (e.kind === "tool_call") stage(e.session, { label: e.label || e.name, at: Date.now() });
  else if (e.kind === "done" || e.kind === "error" || e.kind === "paused") stage(e.session, null);
}

export type SessionMark = "running" | "waiting" | "done" | "failed";
export const sessionMarks = writable<Record<string, SessionMark>>({});

export function watchActivity(): () => void {
  const un = onRunEvent((e) => {
    trackActivity(e);
    trackLog(e);
  });
  return () => {
    un.then((f) => f()).catch(() => {});
  };
}
