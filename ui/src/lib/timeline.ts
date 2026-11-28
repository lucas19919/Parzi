import type { ChatEvent } from "./api";
import { describeTool, type ToolKind, type ToolWords } from "./toolWords.ts";

/** One step inside a group. */
export interface ToolItem extends ToolWords {
  id: string;
  running: boolean;
  ok?: boolean;
  ms?: number;
  output?: string;
}

/** What the thread shows, in the order it happened. Saved events and live
 *  ones go through the same `buildTurns`, so the end of a turn changes nothing
 *  on screen. A block's key is the index of the event that began it. */
export type Block =
  | { key: string; kind: "user"; text: string; at?: number }
  | { key: string; kind: "text"; text: string; at?: number }
  | { key: string; kind: "thinking"; text: string }
  | { key: string; kind: "group"; tool: ToolKind; items: ToolItem[] }
  /** An agent's echo of a tool call, said as text. */
  | { key: string; kind: "echo"; text: string }
  | { key: string; kind: "stop"; at?: number }
  | { key: string; kind: "error"; class: string; message: string; at?: number }
  | { key: string; kind: "widget"; fence: string; payload: unknown }
  | { key: string; kind: "artifact"; id: string; title: string; artifact_kind: string; version: number; payload: unknown; at?: number };

export interface Turn {
  key: string;
  /** The person's message that opened it; the first turn may have none. */
  user: Extract<Block, { kind: "user" }> | null;
  blocks: Block[];
  /** Seconds from the message to the last thing saved, or 0. */
  secs: number;
  steps: number;
  stopped: boolean;
}

/** Fences that are kept but not shown (old plan, task and tidy cards). */
const UNSHOWN = new Set(["parzi-todo", "parzi-task", "parzi-tidy"]);

function isStop(text: string): boolean {
  return /^stopped$/i.test(text.trim());
}

/** An agent that writes its tool call out as text ("read_file({...})"). */
function echoLine(text: string): string {
  const t = text.trim();
  if (/^[\w.]+\(\s*['"`{]/.test(t) || (t.length < 500 && /"(session_id|response)"\s*:/.test(t))) {
    const line = t.split("\n")[0].trim();
    return line.length > 140 ? `${line.slice(0, 140)}…` : line;
  }
  return "";
}

/** Splits events into turns: each of the person's messages opens one.
 *  `base` is the index of `events[0]` in the whole thread, so keys stay the
 *  same when a thread is built in pieces. */
export function buildTurns(events: ChatEvent[], base = 0): Turn[] {
  const turns: Turn[] = [];
  let turn: Turn | null = null;
  const items = new Map<string, ToolItem>();
  const results = new Map<string, Extract<ChatEvent, { kind: "tool_result" }>>();
  for (const e of events) if (e.kind === "tool_result") results.set(e.id, e);
  let first = 0;
  let last = 0;

  const close = () => {
    if (!turn) return;
    turn.secs = first && last > first ? last - first : 0;
    turns.push(turn);
  };
  const open = (key: string, user: Turn["user"]) => {
    close();
    turn = { key, user, blocks: [], secs: 0, steps: 0, stopped: false };
    first = user?.at ?? 0;
    last = first;
  };
  /** Blocks since the last words or card: where thinking and steps gather. */
  const run = (t: Turn): Block[] => {
    let i = t.blocks.length;
    while (i > 0 && (t.blocks[i - 1].kind === "group" || t.blocks[i - 1].kind === "thinking")) i--;
    return t.blocks.slice(i);
  };

  events.forEach((e, n) => {
    const key = `e${base + n}`;
    const at = "at" in e ? e.at : undefined;
    if (e.kind === "user") {
      open(key, { key, kind: "user", text: e.text, at });
      return;
    }
    if (!turn) open(`t${key}`, null);
    const t = turn as unknown as Turn;
    if (at) {
      if (!first) first = at;
      last = Math.max(last, at);
    }
    const tail = t.blocks[t.blocks.length - 1];
    switch (e.kind) {
      case "assistant": {
        const echo = echoLine(e.text);
        if (echo) t.blocks.push({ key, kind: "echo", text: echo });
        else if (tail?.kind === "text") tail.text += e.text;
        else if (e.text.trim()) t.blocks.push({ key, kind: "text", text: e.text, at });
        return;
      }
      case "reasoning": {
        if (!e.text.trim()) return;
        const thought = run(t).find((b) => b.kind === "thinking");
        if (thought && thought.kind === "thinking") {
          if (!thought.text.includes(e.text)) thought.text = `${thought.text}\n\n${e.text}`;
        } else t.blocks.push({ key, kind: "thinking", text: e.text });
        return;
      }
      case "tool_call": {
        if (items.has(e.id)) return;
        const words = describeTool(e.name, e.args);
        const r = results.get(e.id);
        const item: ToolItem = { id: e.id, ...words, running: !r, ok: r?.ok, ms: r?.ms, output: r?.output };
        items.set(e.id, item);
        t.steps++;
        const group = [...run(t)].reverse().find((b) => b.kind === "group");
        if (group && group.kind === "group" && group.tool === words.kind) group.items.push(item);
        else t.blocks.push({ key, kind: "group", tool: words.kind, items: [item] });
        return;
      }
      case "tool_result":
      case "checkpoint":
        return;
      case "system":
        if (isStop(e.text)) {
          t.stopped = true;
          t.blocks.push({ key, kind: "stop", at });
        }
        return;
      case "error":
        t.blocks.push({ key, kind: "error", class: e.class, message: e.message, at });
        return;
      case "widget":
        if (!UNSHOWN.has(e.fence)) t.blocks.push({ key, kind: "widget", fence: e.fence, payload: e.payload });
        return;
      case "artifact":
        t.blocks.push({ key, kind: "artifact", id: e.id, title: e.title, artifact_kind: e.artifact_kind, version: e.version, payload: e.payload, at });
        return;
    }
  });
  close();
  return turns;
}

/** Index of the event that opens the last turn (its message), or 0. */
export function lastTurnStart(events: ChatEvent[]): number {
  for (let i = events.length - 1; i >= 0; i--) if (events[i].kind === "user") return i;
  return 0;
}

/** The turn's words for its footer: "Worked 2m · 12 steps", "Stopped after 4s". */
export function workedLine(t: Turn): string {
  const took = t.secs >= 1 ? spent(t.secs) : "";
  if (t.stopped) return took ? `Stopped after ${took}` : "Stopped";
  if (!t.steps) return "";
  const head = took ? `Worked ${took}` : "Worked";
  return `${head} · ${t.steps} step${t.steps === 1 ? "" : "s"}`;
}

/** "42s", "2m", "1h 5m"; `fine` keeps the seconds past a minute. */
function spent(secs: number, fine = false): string {
  const s = Math.max(0, Math.round(secs));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return fine ? `${m}m ${s % 60}s` : `${m}m`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

/** The last text block of a turn: the reply its footer copies. */
export function replyOf(t: Turn): Extract<Block, { kind: "text" }> | null {
  for (let i = t.blocks.length - 1; i >= 0; i--) {
    const b = t.blocks[i];
    if (b.kind === "text") return b;
  }
  return null;
}
