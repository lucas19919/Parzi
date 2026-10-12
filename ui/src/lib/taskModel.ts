// Pure helpers for the task list: columns, filters, needs and ordering.
// The backend owns the rules (see parzi-core tasks.rs); these only arrange.
import type { Task, TaskStatus } from "./api";

export type Column = "todo" | "doing" | "review" | "done";

export const COLUMNS: { id: Column; label: string }[] = [
  { id: "todo", label: "To do" },
  { id: "doing", label: "Doing" },
  { id: "review", label: "Review" },
  { id: "done", label: "Done" },
];

export const STATUSES: { id: TaskStatus; label: string }[] = [
  { id: "todo", label: "To do" },
  { id: "doing", label: "Doing" },
  { id: "review", label: "Review" },
  { id: "done", label: "Done" },
  { id: "failed", label: "Failed" },
];

// The filter value for every project; "" is the Inbox, anything else a slug.
export const ALL = "*";

// Failed tasks wait in Review: both need a person to look.
export function columnOf(s: TaskStatus): Column {
  return s === "failed" ? "review" : s;
}

export function statusLabel(s: TaskStatus): string {
  return STATUSES.find((x) => x.id === s)?.label ?? s;
}

export function sameProject(a: string | null, b: string | null): boolean {
  return (a ?? "").toLowerCase() === (b ?? "").toLowerCase();
}

export function inScope(t: Task, scope: string): boolean {
  if (scope === ALL) return true;
  return sameProject(t.project, scope || null);
}

// The scope a filter value creates new tasks in (All creates in the Inbox).
export function projectFor(scope: string): string | null {
  return scope === ALL || scope === "" ? null : scope;
}

export function byOrder(a: Task, b: Task): number {
  return a.order - b.order || a.created - b.created;
}

// The board: each column in its order, Done newest first.
export function board(list: Task[], scope: string): Record<Column, Task[]> {
  const out: Record<Column, Task[]> = { todo: [], doing: [], review: [], done: [] };
  for (const t of list) if (inScope(t, scope)) out[columnOf(t.status)].push(t);
  for (const c of ["todo", "doing", "review"] as Column[]) out[c].sort(byOrder);
  out.done.sort((a, b) => b.updated - a.updated);
  return out;
}

export function projectLabel(p: string | null, titles: Map<string, string> = new Map()): string {
  if (p === null) return "Inbox";
  return titles.get(p.toLowerCase()) ?? p;
}

// "Waits on Write the parser and 1 more" for a blocked task, else "".
export function waitsOn(t: Task, all: Task[]): string {
  if (!t.blocked_by.length) return "";
  const names = t.blocked_by.map((id) => all.find((x) => x.id === id)?.title ?? id);
  const first = names[0].length > 40 ? `${names[0].slice(0, 39)}…` : names[0];
  return names.length > 1 ? `Waits on ${first} and ${names.length - 1} more` : `Waits on ${first}`;
}

// What Home shows: what came back for review (newest first), then what is ready.
export function homeTasks(list: Task[], max = 4): Task[] {
  const back = list.filter((t) => t.status === "review" || t.status === "failed").sort((a, b) => b.updated - a.updated);
  const ready = list.filter((t) => t.ready && t.status === "todo").sort(byOrder);
  return [...back, ...ready].slice(0, max);
}

// One step along the columns, or null at the end. Failed counts as Review.
export function nextStatus(s: TaskStatus, dir: 1 | -1): TaskStatus | null {
  const order: Column[] = ["todo", "doing", "review", "done"];
  const at = order.indexOf(columnOf(s));
  const to = order[at + dir];
  return to ?? null;
}

// The column's ids with `id` moved one place up (-1) or down (1), or null.
export function moved(column: Task[], id: string, delta: 1 | -1): string[] | null {
  const ids = column.map((t) => t.id);
  const at = ids.indexOf(id);
  const to = at + delta;
  if (at < 0 || to < 0 || to >= ids.length) return null;
  [ids[at], ids[to]] = [ids[to], ids[at]];
  return ids;
}

// The column's ids with `id` dropped before `before` (or at the end).
export function dropped(column: Task[], id: string, before: string | null): string[] {
  const ids = column.map((t) => t.id).filter((x) => x !== id);
  const at = before ? ids.indexOf(before) : -1;
  ids.splice(at < 0 ? ids.length : at, 0, id);
  return ids;
}

// Tasks `t` may need: same project, not itself, and nothing that already
// waits on it (directly or further down), which would close a circle.
export function needCandidates(t: Task, all: Task[]): Task[] {
  const mine = all.filter((x) => sameProject(x.project, t.project));
  const after = new Set<string>([t.id]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const x of mine) {
      if (!after.has(x.id) && x.needs.some((n) => after.has(n))) {
        after.add(x.id);
        grew = true;
      }
    }
  }
  return mine.filter((x) => !after.has(x.id)).sort(byOrder);
}

export interface ScopeOption {
  value: string;
  label: string;
  count: number;
}

// The project filter: All, the Inbox, then every project with tasks or a folder.
export function scopeOptions(list: Task[], projects: { slug: string; title: string }[]): ScopeOption[] {
  const open = (t: Task) => t.status !== "done";
  const slugs = new Map<string, string>();
  for (const p of projects) slugs.set(p.slug.toLowerCase(), p.slug);
  for (const t of list) if (t.project && !slugs.has(t.project.toLowerCase())) slugs.set(t.project.toLowerCase(), t.project);
  const titles = new Map(projects.map((p) => [p.slug.toLowerCase(), p.title]));
  const rows = [...slugs.values()]
    .map((slug) => ({ value: slug, label: projectLabel(slug, titles), count: list.filter((t) => open(t) && sameProject(t.project, slug)).length }))
    .sort((a, b) => a.label.localeCompare(b.label));
  return [
    { value: ALL, label: "All projects", count: list.filter(open).length },
    { value: "", label: "Inbox", count: list.filter((t) => open(t) && t.project === null).length },
    ...rows,
  ];
}
