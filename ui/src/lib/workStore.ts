// Work, live: one copy of the work view for the Work tab, Home, the dock and
// the session strip, refreshed whenever the ledger changes; plus what each
// agent is doing right now, read off the run events and never stored.
import { get, readable, writable } from "svelte/store";
import { onRunEvent, onWork, workApi, type GoalStatus, type UiEvent, type WorkChange, type WorkView } from "./api";
import { coalesce } from "./threadList";
import { chime, notifyAction } from "./toast";
import type { Activity } from "./workModel";

const EMPTY: WorkView = { goals: [], asks: [], digest: null, server: { linked: false, ops: true } };

export const work = writable<WorkView>(EMPTY);
export const workLoaded = writable(false);
export const workError = writable("");

export const refreshWork = coalesce(async () => {
  try {
    work.set(await workApi.view());
    workError.set("");
  } catch (e) {
    workError.set(String(e));
  }
  workLoaded.set(true);
}, 80);

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

// A clock for elapsed times on cards.
export const now = readable(Date.now(), (set) => {
  const timer = setInterval(() => set(Date.now()), 10_000);
  return () => clearInterval(timer);
});

// The session the window shows, so a goal you are watching finish does not
// also toast.
export const watching = writable<string | null>(null);

// Opening the Work tab, at a goal: App watches the counter (like Brain).
export const targetGoal = writable("");
export const workTabRequested = writable(0);

export function openGoal(id = "") {
  targetGoal.set(id);
  workTabRequested.update((n) => n + 1);
}

// The lead session's run end and its goal settling arrive separately, in
// either order: whichever toasts first marks the session so the other
// stays quiet.
const toastedAt = new Map<string, number>();

export function markToasted(session: string) {
  const t = Date.now();
  for (const [s, at] of toastedAt) if (t - at > 60_000) toastedAt.delete(s);
  toastedAt.set(session, t);
}

export function toastedRecently(session: string): boolean {
  const at = toastedAt.get(session);
  return at !== undefined && Date.now() - at < 10_000;
}

function short(title: string) {
  return title.length > 48 ? `${title.slice(0, 47)}…` : title;
}

// A budget pause toasts from its run event already (App), so it is not here.
const SETTLED: Partial<Record<GoalStatus, string>> = {
  done: "Done",
  check: "Check this",
  needs_you: "Waiting for you",
  failed: "Didn't finish",
  stopped: "Stopped",
  cut_off: "Cut off",
};

function settle(list: WorkChange["settled"]) {
  const goals = get(work).goals;
  const shown = get(watching);
  for (const s of list) {
    const word = SETTLED[s.status];
    if (!word) continue;
    const root = goals.find((g) => g.id === s.goal)?.root ?? null;
    if (root && (root === shown || toastedRecently(root))) continue;
    if (root) markToasted(root);
    notifyAction(`${word}: ${short(s.title)}`, () => openGoal(s.goal), s.status === "failed");
    chime(s.status === "done");
  }
}

// Live updates, the activity line, and a toast (click to open the goal)
// when a goal's lead run ends.
export function watchWork(): () => void {
  void refreshWork();
  const unWork = onWork((c) => {
    void refreshWork().then(() => settle(c.settled ?? []));
  });
  const unRun = onRunEvent(trackActivity);
  return () => {
    for (const un of [unWork, unRun]) un.then((f) => f()).catch(() => {});
  };
}
