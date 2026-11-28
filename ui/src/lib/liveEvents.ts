import type { ChatEvent } from "./api";

/** What a running turn has sent that the store may not have yet, in the
 *  same shape as saved events, so one timeline draws both. */
export interface Live {
  events: ChatEvent[];
  /** Reply text received but not shown yet: it is let out at an even pace. */
  pending: string;
  /** How much of the unsaved reply has been received, in UTF-16 units. The
   *  window reads this off each text event, so text it already has (from
   *  `get_thread` while the reply was being written) is skipped. */
  upto: number;
}

export const emptyLive = (): Live => ({ events: [], pending: "", upto: 0 });

const now = () => Math.floor(Date.now() / 1000);

function withText(events: ChatEvent[], text: string): ChatEvent[] {
  if (!text) return events;
  const last = events[events.length - 1];
  if (last?.kind === "assistant") return [...events.slice(0, -1), { ...last, text: last.text + text }];
  return [...events, { kind: "assistant", text, done: false, at: now() }];
}

/** Reply text arrived. `upto` (when the host sends it) is how long the
 *  unsaved reply is with this piece. */
export function addText(s: Live, text: string, upto?: number): Live {
  if (typeof upto !== "number") return { ...s, pending: s.pending + text };
  if (upto <= s.upto) return s;
  const start = upto - text.length;
  const fresh = start < s.upto ? text.slice(s.upto - start) : text;
  return { ...s, pending: s.pending + fresh, upto };
}

/** Lets `n` characters of the waiting text onto the screen. */
export function reveal(s: Live, n: number): Live {
  if (!s.pending) return s;
  let cut = Math.min(s.pending.length, Math.max(1, n));
  if (cut < s.pending.length && /[\uD800-\uDBFF]/.test(s.pending[cut - 1])) cut++;
  return { ...s, events: withText(s.events, s.pending.slice(0, cut)), pending: s.pending.slice(cut) };
}

/** Shows all waiting text at once: something else is about to follow it. */
export function settle(s: Live): Live {
  return s.pending ? { ...s, events: withText(s.events, s.pending), pending: "" } : s;
}

export function addReasoning(s: Live, text: string): Live {
  const t = settle(s);
  const last = t.events[t.events.length - 1];
  if (last?.kind === "reasoning") return { ...t, events: [...t.events.slice(0, -1), { ...last, text: last.text + text }] };
  return { ...t, events: [...t.events, { kind: "reasoning", text, at: now() }] };
}

/** A step began. The words before it are saved with it, so the next reply
 *  counts from nothing. */
export function addCall(s: Live, id: string, name: string, args: unknown): Live {
  const t = settle(s);
  if (t.events.some((e) => e.kind === "tool_call" && e.id === id)) return t;
  return { ...t, upto: 0, events: [...t.events, { kind: "tool_call", id, name, args: args ?? {}, at: now() }] };
}

export function addResult(s: Live, id: string, name: string, ok: boolean, ms: number, output: string): Live {
  if (s.events.some((e) => e.kind === "tool_result" && e.id === id)) return s;
  return { ...s, events: [...s.events, { kind: "tool_result", id, name, ok, ms, output: output ?? "", at: now() }] };
}

/** Anything else the store records in order (an artifact, the hidden plan card). */
export function addEvent(s: Live, e: ChatEvent): Live {
  const t = settle(s);
  if (e.kind === "artifact") {
    const kept = t.events.filter((x) => !(x.kind === "artifact" && x.id === e.id && x.version === e.version));
    return { ...t, events: [...kept, e] };
  }
  return { ...t, events: [...t.events, e] };
}

/** A reply `get_thread` returned that the store does not have yet. */
export function isUnsaved(e: ChatEvent | undefined): boolean {
  return !!e && e.kind === "assistant" && (e as { unsaved?: boolean }).unsaved === true;
}

/**
 * A fresh read of the store arrived while a turn runs. Returns the saved
 * events to show and what of the live turn is still not in them:
 * - steps the store has are dropped from the live turn, with everything said
 *   before the last of them;
 * - thinking the store already has is dropped;
 * - the unsaved reply it returned becomes the live reply, with anything
 *   that arrived after the read kept on its end.
 */
export function reconcile(read: ChatEvent[], s: Live): { stored: ChatEvent[]; live: Live } {
  const unsaved = isUnsaved(read[read.length - 1]) ? (read[read.length - 1] as Extract<ChatEvent, { kind: "assistant" }>) : null;
  const stored = unsaved ? read.slice(0, -1) : read;
  const saved = new Set(stored.flatMap((e) => (e.kind === "tool_call" || e.kind === "tool_result" ? [`${e.kind}:${e.id}`] : [])));
  let events = s.events;
  let cut = -1;
  events.forEach((e, i) => {
    if ((e.kind === "tool_call" || e.kind === "tool_result") && saved.has(`${e.kind}:${e.id}`)) cut = i;
  });
  if (cut >= 0) events = events.slice(cut + 1).filter((e) => !((e.kind === "tool_call" || e.kind === "tool_result") && saved.has(`${e.kind}:${e.id}`)));
  // Thinking the store wrote down since its last step.
  let lastStep = stored.length - 1;
  while (lastStep >= 0 && stored[lastStep].kind !== "tool_call" && stored[lastStep].kind !== "tool_result" && stored[lastStep].kind !== "user") lastStep--;
  const thoughts = stored.slice(lastStep + 1).flatMap((e) => (e.kind === "reasoning" ? [e.text.trim()] : []));
  events = events.filter((e) => !(e.kind === "reasoning" && thoughts.some((t) => t.startsWith(e.text.trim()))));
  let live: Live = { ...s, events };
  if (unsaved) {
    // The host counts the whole unsaved reply; it sends only what the store lacks.
    const have = (unsaved as { upto?: number }).upto ?? unsaved.text.length;
    const text = events.length && events[events.length - 1].kind === "assistant" ? (events[events.length - 1] as { text: string }).text : "";
    // What this window received of the reply covers [upto - shown - pending, upto).
    const got = text + s.pending;
    const from = s.upto - got.length;
    const after = s.upto > have ? got.slice(Math.max(0, have - from)) : "";
    const rest = text ? events.slice(0, -1) : events;
    live = { events: [...rest, { kind: "assistant", text: unsaved.text, done: false, at: unsaved.at ?? now() }], pending: after, upto: Math.max(have, s.upto) };
  }
  return { stored, live };
}
