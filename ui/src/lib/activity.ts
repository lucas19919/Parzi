// What each agent is doing right now, read off the run events and never
// stored: the plan panel's live lines.
import { writable } from "svelte/store";
import { onRunEvent, type UiEvent } from "./api";

export interface Activity {
  label: string;
  at: number;
}

// Session id (tagged, so server sessions too) -> its latest tool call while
// it runs. Cleared when the run ends or pauses.
export const activity = writable<Map<string, Activity>>(new Map());

// Tool calls can come many a second: apply them at most every 150 ms.
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
  // A finished tool keeps its line until the next call: the agent is still
  // on that, thinking about the result.
  if (e.kind === "tool_call") stage(e.session, { label: e.label || e.name, at: Date.now() });
  else if (e.kind === "done" || e.kind === "error" || e.kind === "paused") stage(e.session, null);
}

// Sessions working, or waiting on the person: their tabs say so, so a
// question in a background tab is not missed.
export type SessionMark = "running" | "waiting";
export const sessionMarks = writable<Record<string, SessionMark>>({});

export function watchActivity(): () => void {
  const un = onRunEvent(trackActivity);
  return () => {
    un.then((f) => f()).catch(() => {});
  };
}
