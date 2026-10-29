import { writable } from "svelte/store";
import { onRunEvent, type UiEvent } from "./api";

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

export function trackActivity(e: UiEvent) {
  if (e.kind === "tool_call") stage(e.session, { label: e.label || e.name, at: Date.now() });
  else if (e.kind === "done" || e.kind === "error" || e.kind === "paused") stage(e.session, null);
}

export type SessionMark = "running" | "waiting";
export const sessionMarks = writable<Record<string, SessionMark>>({});

export function watchActivity(): () => void {
  const un = onRunEvent(trackActivity);
  return () => {
    un.then((f) => f()).catch(() => {});
  };
}
