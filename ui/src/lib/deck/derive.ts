/**
 * Everything the deck computes from the files the commands return
 * (PROJECT.md, PLAN.md, journal.jsonl). Pure and type-only imports, so the
 * node test runner can check it without Tauri: same inputs, same cards.
 */
import type { JournalLine, Plan, Project, Task } from "../api";

export type TaskState = "pending" | "claimed" | "running" | "blocked" | "done";

/** The live overlay on one plan task, derived from the journal. */
export interface TaskLive {
  state: TaskState;
  /** Who holds the task while it is claimed/running. */
  holder: string;
  /** Tool the lane is in, when its last Note said so (`tool:read turn:3`). */
  tool: string;
  turn: number;
  /** A `lease.request` for this task is waiting for an answer. */
  requestPending: boolean;
  /** Last journal text for the card's tooltip. */
  note: string;
}

const EMPTY: TaskLive = { state: "pending", holder: "", tool: "", turn: 0, requestPending: false, note: "" };

/** `tool:<name> turn:<n>` in a Note line: the lane's heartbeat until H2. */
const HEARTBEAT = /\btool:([\w.\-]+)(?:\s+turn:(\d+))?/;

/** The state a task has before any journal line touches it. */
export function restingState(t: Task): TaskLive {
  return { ...EMPTY, state: t.done ? "done" : "pending" };
}

/**
 * Task id → live state. Journal lines are applied in order, so a Release
 * after a Claim is pending again and a Handoff always wins.
 */
export function taskStates(plan: Plan, journal: JournalLine[]): Record<string, TaskLive> {
  const out: Record<string, TaskLive> = {};
  for (const t of allTasks(plan)) out[t.id] = restingState(t);
  for (const line of journal) {
    const id = line.task;
    if (!id) continue;
    const cur = out[id] ?? { ...EMPTY };
    out[id] = cur;
    cur.note = line.text;
    switch (line.kind) {
      case "claim":
        cur.state = "claimed";
        cur.holder = line.who;
        break;
      case "release":
        if (cur.state !== "done") cur.state = "pending";
        cur.holder = "";
        cur.tool = "";
        cur.turn = 0;
        break;
      case "block":
        cur.state = "blocked";
        break;
      case "handoff":
        cur.state = "done";
        cur.tool = "";
        cur.requestPending = false;
        break;
      case "request":
        cur.requestPending = true;
        break;
      case "grant":
      case "deny":
        cur.requestPending = false;
        break;
      case "note": {
        const m = HEARTBEAT.exec(line.text);
        if (m && cur.state !== "done") {
          cur.state = "running";
          cur.tool = m[1];
          cur.turn = Number(m[2] ?? 0);
          if (!cur.holder) cur.holder = line.who;
        }
        break;
      }
      default:
        break;
    }
  }
  return out;
}

export function allTasks(plan: Plan): Task[] {
  return plan.sprints.flatMap((s) => s.lanes.flatMap((l) => l.tasks));
}

/** Column order of the grid: every lane name in the plan, first seen first. */
export function laneNames(plan: Plan): string[] {
  const out: string[] = [];
  for (const s of plan.sprints) {
    for (const l of s.lanes) if (!out.includes(l.name)) out.push(l.name);
  }
  return out;
}

/** Sprint n/m for the header: the first sprint with an unfinished task. */
export function sprintPosition(plan: Plan, live: Record<string, TaskLive>): { n: number; m: number } {
  const m = plan.sprints.length;
  for (let i = 0; i < m; i++) {
    const tasks = plan.sprints[i].lanes.flatMap((l) => l.tasks);
    if (tasks.some((t) => (live[t.id]?.state ?? (t.done ? "done" : "pending")) !== "done")) {
      return { n: i + 1, m };
    }
  }
  return { n: m, m };
}

export function liveLaneCount(live: Record<string, TaskLive>): number {
  return Object.values(live).filter((l) => l.state === "running" || l.state === "claimed").length;
}

const role = (m: string) => m || "auto";

/**
 * PROJECT.md rebuilt from the parsed project, so the Project tab renders one
 * markdown document (md.ts) and not a form.
 *
 * The header block is two rows — roster and critical. Workspace, repos,
 * budget and status are not repeated here: they are the pills above the
 * document and the deck header, and as six more `key: value` lines they
 * were the paragraph of run-together text the review found.
 *
 * `full = false` is the collapsed reading the Project tab opens with: Why
 * and What, the two sections a person actually reads.
 */
export function projectMarkdown(p: Project, full = true): string {
  const out: string[] = [];
  if (full) {
    out.push(`# Project: ${p.title}`, "");
    out.push(
      `roster: header = ${role(p.roster.header)}, orchestrator = ${role(p.roster.orchestrator)}, coder = ${role(p.roster.coder)}`,
    );
    out.push(`critical: ${p.critical.length ? p.critical.join(", ") : "nothing marked"}`);
  }
  if (p.why.trim()) out.push("", "## Why", "", p.why.trim());
  if (p.what.length) {
    out.push("", "## What", "");
    for (const c of p.what) out.push(`- [${c.done ? "x" : " "}] ${c.text}`);
  }
  if (full && p.constraints.length) {
    out.push("", "## Constraints", "");
    for (const c of p.constraints) out.push(`- ${c}`);
  }
  return out.join("\n").replace(/^\n+/, "") + "\n";
}

/**
 * Cheap change key: the poll re-renders only when this differs. Commands
 * that carry a file mtime (`{text, etag}`) hand theirs in directly.
 */
export function etagOf(v: unknown): string {
  if (v && typeof v === "object" && typeof (v as { etag?: unknown }).etag === "string") {
    return (v as { etag: string }).etag;
  }
  const s = JSON.stringify(v) ?? "";
  let h = 5381;
  for (let i = 0; i < s.length; i++) h = ((h * 33) ^ s.charCodeAt(i)) >>> 0;
  return `${s.length}:${h.toString(36)}`;
}
