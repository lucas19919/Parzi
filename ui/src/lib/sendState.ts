// One send state per session: whether a message is being handed to the
// engine, a run is going or being stopped, what waits in the queue, and the
// message kept on screen until the store has it. App.svelte feeds it events
// and carries out the effects it returns; nothing else tracks sending.
import { dropId, enqueue, heldId, moveHeld, moveId, putFront, takeAll, takeHead, takeId, type HeldItem, type HoldQueue } from "./held.ts";

/**
 * idle: nothing of ours is going. sending: a message is with the engine and
 * the run is not confirmed yet. running: a run is going. stopping: a stop was
 * asked and the engine has not let go of the run yet.
 */
export type Phase = "idle" | "sending" | "running" | "stopping";

/** How a run ended. "exited" is an agent that quit before its turn was done. */
export type Outcome = "done" | "failed" | "exited" | "paused" | "stopped";

/** The message just sent, shown until the store holds more than `users` user messages. */
export interface Echo {
  text: string;
  users: number;
}

export interface SessionSend {
  phase: Phase;
  /** When an event (not a store read) last changed the phase. */
  since: number;
  echo: Echo | null;
  /** "Send now": goes out as soon as the stop lands. */
  next: HeldItem | null;
  /** A run end that came while the message was still being handed over. */
  ended: Outcome | null;
  /** Retries after the engine said it was still working; 0 when none is due. */
  tries: number;
  /** Until when a late "cancelled" end belongs to the run just stopped, not to the next one. */
  stopEndBy: number;
}

export interface Sends {
  sessions: Record<string, SessionSend>;
  queue: HoldQueue;
  paused: Record<string, true>;
}

export type Hold = Omit<HeldItem, "id"> & { id?: string };

export type SendEvent =
  /** Enter while the session is busy: the message joins the queue. */
  | { type: "hold"; key: string; item: Hold }
  /** A typed message goes to the engine now. */
  | { type: "begin"; key: string; text: string; users: number }
  /** What to show for a queued message being handed over. */
  | { type: "echo"; key: string; text: string; users: number }
  /** The engine took the message; `sid` is the session it runs in. */
  | { type: "started"; key: string; sid: string }
  /** The engine refused the message. `item` goes back to the queue: at the front with `front`, else at the end. */
  | { type: "failed"; key: string; item?: Hold; front?: boolean; stillWorking: boolean }
  /** Any run event: the session is working, whoever started it. */
  | { type: "activity"; key: string }
  | { type: "ended"; key: string; outcome: Outcome }
  | { type: "stop"; key: string }
  /** The engine's stop returned (or failed): the run is gone. */
  | { type: "stopped"; key: string }
  | { type: "sendNow"; key: string; itemId?: string }
  /** Retry `tries` came due; `status` is the store's word for the session then. */
  | { type: "woke"; key: string; tries: number; status: string }
  /** Session statuses read from the store. */
  | { type: "seen"; list: { id: string; status: string }[] }
  /** The store holds `users` user messages for the session. */
  | { type: "stored"; key: string; users: number }
  | { type: "take"; key: string; itemId?: string }
  | { type: "putBack"; key: string; item: HeldItem }
  | { type: "drop"; key: string; itemId: string }
  | { type: "move"; key: string; itemId: string; to: number }
  | { type: "pause"; key: string; on: boolean }
  | { type: "deleted"; key: string };

export type Effect =
  /** Hand this queued message to the engine (the session is already "sending"). */
  | { do: "deliver"; key: string; item: HeldItem }
  | { do: "kill"; key: string }
  /** Check again in `ms` and report with "woke", passing `tries` back. A
   *  later retry or run end makes an earlier one a no-op, so timers need no
   *  cancelling. */
  | { do: "retry"; key: string; ms: number; tries: number };

export interface Step {
  state: Sends;
  effects: Effect[];
  /** False when the event did nothing: a full queue, nothing to stop, no such item. */
  ok: boolean;
  /** The item taken by "take". */
  item?: HeldItem;
}

/** The store lags behind run events: a read this soon after one doesn't overrule it. */
export const GRACE_MS = 2000;
/** Waits between tries while the engine is still letting go of the last run. */
export const RETRY_MS = [250, 500, 1000, 1500, 2000, 2000];

/** The key for a new session's tab before it has a session id. */
export const tabKey = (tabId: string) => `tab:${tabId}`;

const IDLE: SessionSend = { phase: "idle", since: -Infinity, echo: null, next: null, ended: null, tries: 0, stopEndBy: 0 };

export function initialSends(queue: HoldQueue = {}): Sends {
  return { sessions: {}, queue, paused: {} };
}

export function sessionOf(s: Sends, key: string): SessionSend {
  return s.sessions[key] ?? IDLE;
}

export const phaseOf = (s: Sends, key: string): Phase => sessionOf(s, key).phase;

/** Enter queues instead of sending: something is going, or a retry is due. */
export function isBusy(s: Sends, key: string): boolean {
  const cur = sessionOf(s, key);
  return cur.phase !== "idle" || cur.tries > 0;
}

/** At work, for the stop button and the tab's state icon. A stop being carried out no longer counts. */
export function isWorking(s: Sends, key: string): boolean {
  const p = phaseOf(s, key);
  return p === "sending" || p === "running";
}

export function workingSet(s: Sends): Set<string> {
  return new Set(Object.keys(s.sessions).filter((k) => !k.startsWith("tab:") && isWorking(s, k)));
}

export const isPaused = (s: Sends, key: string): boolean => !!s.paused[key];

export const echoOf = (s: Sends, key: string): Echo | null => sessionOf(s, key).echo;

const live = (status: string) => status === "active" || status === "queued";

function put(s: Sends, key: string, patch: Partial<SessionSend>): Sends {
  return { ...s, sessions: { ...s.sessions, [key]: { ...sessionOf(s, key), ...patch } } };
}

/** A pause only means something while there is a queue. */
function withQueue(s: Sends, queue: HoldQueue): Sends {
  if (queue === s.queue) return s;
  const gone = Object.keys(s.paused).filter((k) => !queue[k]?.length);
  if (!gone.length) return { ...s, queue };
  const paused = { ...s.paused };
  for (const k of gone) delete paused[k];
  return { ...s, queue, paused };
}

function setPaused(s: Sends, key: string, on: boolean): Sends {
  if (on === !!s.paused[key] || (on && !s.queue[key]?.length)) return s;
  const paused = { ...s.paused };
  if (on) paused[key] = true;
  else delete paused[key];
  return { ...s, paused };
}

const done = (state: Sends, effects: Effect[] = [], ok = true): Step => ({ state, effects, ok });

/** Sends the head of the queue if nothing holds it back; the session must be free. */
function drain(s: Sends, key: string, effects: Effect[]): Sends {
  if (s.paused[key] || !s.queue[key]?.length) return s;
  const next = takeHead(s.queue, key);
  if (!next.item) return s;
  effects.push({ do: "deliver", key, item: next.item });
  return put(withQueue(s, next.queue), key, { phase: "sending", echo: null, ended: null });
}

// The echo stays: the reload after a run's end is what shows the store has it.
function endRun(s: Sends, key: string, outcome: Outcome, now: number): Step {
  let state = put(s, key, { phase: "idle", since: now, tries: 0, ended: null });
  if (outcome === "exited") state = setPaused(state, key, true);
  const effects: Effect[] = [];
  if (outcome === "done") state = drain(state, key, effects);
  return done(state, effects);
}

/** Moves everything kept for a session to the id the engine gave it. */
function rekey(s: Sends, from: string, to: string): Sends {
  if (from === to) return s;
  const sessions = { ...s.sessions };
  sessions[to] = { ...sessionOf(s, from) };
  delete sessions[from];
  const paused = { ...s.paused };
  if (paused[from]) paused[to] = true;
  delete paused[from];
  return { sessions, queue: moveHeld(s.queue, from, to), paused };
}

export function step(s: Sends, ev: SendEvent, now: number): Step {
  switch (ev.type) {
    case "hold": {
      const r = enqueue(s.queue, ev.key, ev.item);
      return done(withQueue(s, r.queue), [], r.ok);
    }
    case "begin": {
      if (isBusy(s, ev.key)) return done(s, [], false);
      return done(put(s, ev.key, { phase: "sending", since: now, echo: { text: ev.text, users: ev.users }, ended: null }));
    }
    case "echo": {
      if (phaseOf(s, ev.key) !== "sending") return done(s, [], false);
      return done(put(s, ev.key, { echo: { text: ev.text, users: ev.users } }));
    }
    case "started": {
      const state = rekey(s, ev.key, ev.sid);
      const cur = sessionOf(state, ev.sid);
      // A stop asked while the message was on its way still goes ahead.
      if (cur.phase === "stopping") return done(state);
      if (cur.ended) return endRun(state, ev.sid, cur.ended, now);
      return done(put(state, ev.sid, { phase: "running", since: now, tries: 0, ended: null }));
    }
    case "failed": {
      const cur = sessionOf(s, ev.key);
      let state = s;
      let ok = true;
      if (ev.item && (ev.front || ev.stillWorking)) {
        if (ev.front) {
          state = withQueue(state, putFront(state.queue, ev.key, { ...ev.item, id: ev.item.id || heldId() }));
        } else {
          const r = enqueue(state.queue, ev.key, ev.item);
          state = withQueue(state, r.queue);
          ok = r.ok;
        }
      }
      if (cur.phase === "stopping") return done(put(state, ev.key, { echo: null }), [], ok);
      if (!ev.stillWorking) return done(put(state, ev.key, { phase: "idle", since: now, echo: null, ended: null }), [], ok);
      // The engine is still letting go of the last run: count it as working
      // and try the queue again shortly.
      const tries = cur.tries + 1;
      const effects: Effect[] = [];
      const due = tries <= RETRY_MS.length && !!state.queue[ev.key]?.length && !state.paused[ev.key];
      if (due) effects.push({ do: "retry", key: ev.key, ms: RETRY_MS[tries - 1], tries });
      return done(put(state, ev.key, { phase: "running", since: now, echo: null, ended: null, tries: due ? tries : 0 }), effects, ok);
    }
    case "activity": {
      if (phaseOf(s, ev.key) !== "idle") return done(s);
      return done(put(s, ev.key, { phase: "running", since: now }));
    }
    case "ended": {
      const cur = sessionOf(s, ev.key);
      // The stop's own end, after the engine's stop had already returned.
      if (ev.outcome === "stopped" && now < cur.stopEndBy) return done(put(s, ev.key, { stopEndBy: 0 }));
      if (cur.phase === "sending") return done(put(s, ev.key, { ended: ev.outcome }));
      // The stop's own end: the stop's return settles it.
      if (cur.phase === "stopping") return done(put(s, ev.key, { ended: ev.outcome }));
      return endRun(s, ev.key, ev.outcome, now);
    }
    case "stop": {
      if (!isWorking(s, ev.key)) return done(s, [], false);
      // A stop never lets the queue go out on its own.
      const state = setPaused(s, ev.key, true);
      return done(put(state, ev.key, { phase: "stopping", since: now, tries: 0 }), [{ do: "kill", key: ev.key }]);
    }
    case "stopped": {
      const cur = sessionOf(s, ev.key);
      if (cur.phase !== "stopping" && cur.phase !== "running") return done(s);
      // Run events can come after the call that made them returns: if the
      // stopped run's end hasn't come yet, it mustn't end what goes next.
      const stopEndBy = cur.phase === "stopping" && !cur.ended ? now + GRACE_MS : 0;
      let state = put(s, ev.key, { phase: "idle", since: now, next: null, ended: null, stopEndBy });
      const effects: Effect[] = [];
      if (cur.next) {
        effects.push({ do: "deliver", key: ev.key, item: cur.next });
        state = put(state, ev.key, { phase: "sending" });
      } else {
        // Only what was typed during the stop: the queue from before is paused.
        state = drain(state, ev.key, effects);
      }
      return done(state, effects);
    }
    case "sendNow": {
      const taken = ev.itemId ? takeId(s.queue, ev.key, ev.itemId) : takeHead(s.queue, ev.key);
      if (!taken.item) return done(s, [], false);
      const item = taken.item;
      let state = setPaused(withQueue(s, taken.queue), ev.key, false);
      const cur = sessionOf(state, ev.key);
      if (cur.phase === "running") {
        return done(put(state, ev.key, { phase: "stopping", since: now, next: item, tries: 0 }), [{ do: "kill", key: ev.key }]);
      }
      if (cur.phase === "stopping") {
        if (cur.next) state = withQueue(state, putFront(state.queue, ev.key, cur.next));
        return done(put(state, ev.key, { next: item }));
      }
      // A message is on its way already: this one goes right after that turn.
      if (cur.phase === "sending") return done(withQueue(state, putFront(state.queue, ev.key, item)));
      return done(put(state, ev.key, { phase: "sending", since: now, echo: null, ended: null, tries: 0 }), [{ do: "deliver", key: ev.key, item }]);
    }
    case "woke": {
      const cur = sessionOf(s, ev.key);
      if (!cur.tries || cur.tries !== ev.tries || (cur.phase !== "idle" && cur.phase !== "running")) return done(s);
      if (live(ev.status)) {
        if (cur.tries >= RETRY_MS.length) return done(put(s, ev.key, { tries: 0 }));
        const tries = cur.tries + 1;
        return done(put(s, ev.key, { tries }), [{ do: "retry", key: ev.key, ms: RETRY_MS[tries - 1], tries }]);
      }
      const effects: Effect[] = [];
      const state = drain(put(s, ev.key, { phase: "idle", since: now }), ev.key, effects);
      return done(effects.length ? state : put(state, ev.key, { tries: 0 }), effects);
    }
    case "seen": {
      let state = s;
      for (const { id, status } of ev.list) {
        const cur = state.sessions[id];
        const on = live(status);
        if (!cur) {
          if (on) state = put(state, id, { phase: "running" });
          continue;
        }
        if (now - cur.since < GRACE_MS) continue;
        if (cur.phase === "idle" && on) state = put(state, id, { phase: "running" });
        else if (cur.phase === "running" && !on) state = put(state, id, { phase: "idle" });
      }
      return done(state);
    }
    case "stored": {
      const echo = echoOf(s, ev.key);
      if (!echo || ev.users <= echo.users) return done(s);
      return done(put(s, ev.key, { echo: null }));
    }
    case "take": {
      const r = ev.itemId ? takeId(s.queue, ev.key, ev.itemId) : takeHead(s.queue, ev.key);
      return { ...done(withQueue(s, r.queue), [], !!r.item), item: r.item };
    }
    case "putBack":
      return done(withQueue(s, putFront(s.queue, ev.key, ev.item)));
    case "drop":
      return done(withQueue(s, dropId(s.queue, ev.key, ev.itemId)));
    case "move":
      return done(withQueue(s, moveId(s.queue, ev.key, ev.itemId, ev.to)));
    case "pause": {
      const state = setPaused(s, ev.key, ev.on);
      if (ev.on || isBusy(state, ev.key)) return done(state);
      const effects: Effect[] = [];
      return done(drain(state, ev.key, effects), effects);
    }
    case "deleted": {
      const sessions = { ...s.sessions };
      delete sessions[ev.key];
      const paused = { ...s.paused };
      delete paused[ev.key];
      return done({ sessions, paused, queue: takeAll(s.queue, ev.key).queue });
    }
  }
}
