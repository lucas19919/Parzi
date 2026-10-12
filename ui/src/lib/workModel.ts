// Pure helpers for Work: bands, status words, the step rail's layout and the
// live activity line. The backend owns the rules (the work ledger in
// parzi-core); these only arrange what it reports.
import type { AskView, Engine, GoalStatus, GoalView, RunView, StepStatus, StepView } from "./api";

export const DAY_MS = 86_400_000;
export const WEEK_MS = 7 * DAY_MS;

export type Band = "needs" | "working" | "later" | "done";

export const BANDS: { id: Band; label: string }[] = [
  { id: "needs", label: "Needs you" },
  { id: "working", label: "Working now" },
  { id: "later", label: "Later" },
  { id: "done", label: "Done recently" },
];

const NEEDS: GoalStatus[] = ["needs_you", "paused", "check", "failed"];
const ENDED: GoalStatus[] = ["done", "stopped", "cut_off"];

export function hasOpenAsk(g: GoalView, asks: AskView[] = []): boolean {
  return g.asks > 0 || asks.some((a) => a.goal === g.id);
}

// Where a goal shows, or null once it is archived or a week old.
export function bandOf(g: GoalView, asks: AskView[] = [], now = Date.now()): Band | null {
  if (g.status === "archived") return null;
  if (NEEDS.includes(g.status) || hasOpenAsk(g, asks)) return "needs";
  if (g.status === "working") return "working";
  if (g.status === "later") return "later";
  if (ENDED.includes(g.status) && now - g.updated <= WEEK_MS) return "done";
  return null;
}

// The bands in their order. Live bands keep a steady order (a ledger append
// must not shuffle the cards): working by when the goal began, Later as
// queued, the rest newest first.
export function groupBands(goals: GoalView[], asks: AskView[] = [], now = Date.now()): Record<Band, GoalView[]> {
  const out: Record<Band, GoalView[]> = { needs: [], working: [], later: [], done: [] };
  for (const g of goals) {
    const b = bandOf(g, asks, now);
    if (b) out[b].push(g);
  }
  out.needs.sort((a, b) => b.updated - a.updated);
  out.working.sort((a, b) => b.created - a.created);
  out.later.sort((a, b) => a.created - b.created);
  out.done.sort((a, b) => b.updated - a.updated);
  return out;
}

const WORDS: Record<GoalStatus, string> = {
  working: "Working",
  needs_you: "Waiting for you",
  paused: "Paused: budget",
  check: "Check this",
  done: "Done",
  failed: "Didn't finish",
  stopped: "Stopped",
  cut_off: "Cut off",
  later: "Later",
  archived: "Archived",
};

export function statusWord(g: Pick<GoalView, "status" | "later" | "asks">): string {
  if (g.status === "later") return g.later?.auto_start ? "Starts when free" : "Later";
  if (g.status === "working" && g.asks > 0) return WORDS.needs_you;
  return WORDS[g.status] ?? g.status;
}

// The colour a status reads in: live work, something for you, a failure,
// finished, or quiet.
export type Tone = "live" | "need" | "bad" | "ok" | "idle";

export function toneOf(g: Pick<GoalView, "status" | "asks">): Tone {
  switch (g.status) {
    case "working":
      return g.asks > 0 ? "need" : "live";
    case "needs_you":
    case "paused":
    case "check":
      return "need";
    case "failed":
      return "bad";
    case "done":
      return "ok";
    default:
      return "idle";
  }
}

export function stepWord(s: Pick<StepView, "status" | "blocked_by">): string {
  switch (s.status) {
    case "done":
      return "Done";
    case "running":
      return "Working";
    case "failed":
      return "Didn't finish";
    case "skipped":
      return "Skipped";
    case "check":
      return "Check this";
    default:
      return s.blocked_by.length ? "Waiting" : "To do";
  }
}

export function stepTone(s: Pick<StepView, "status">): Tone {
  switch (s.status) {
    case "running":
      return "live";
    case "check":
      return "need";
    case "failed":
      return "bad";
    case "done":
      return "ok";
    default:
      return "idle";
  }
}

// "Waits on Make autosize measure once and 1 more" for a blocked step.
export function waitsOn(s: StepView, steps: StepView[]): string {
  if (!s.blocked_by.length || s.status !== "todo") return "";
  const names = s.blocked_by.map((k) => steps.find((x) => x.key === k)?.title || k);
  const first = names[0].length > 48 ? `${names[0].slice(0, 47)}…` : names[0];
  return names.length > 1 ? `Waits on ${first} and ${names.length - 1} more` : `Waits on ${first}`;
}

// Each step's depth: 0 without needs, else one past its deepest need. Needs
// that name no step, and the back edge of a cycle, are left out.
export function stepLayers(steps: StepView[]): Map<string, number> {
  const byKey = new Map(steps.map((s) => [s.key, s]));
  const depth = new Map<string, number>();
  const visiting = new Set<string>();
  const walk = (key: string): number => {
    const known = depth.get(key);
    if (known !== undefined) return known;
    if (visiting.has(key)) return 0;
    visiting.add(key);
    let d = 0;
    for (const n of byKey.get(key)?.needs ?? []) if (n !== key && byKey.has(n)) d = Math.max(d, walk(n) + 1);
    visiting.delete(key);
    depth.set(key, d);
    return d;
  };
  for (const s of steps) walk(s.key);
  return depth;
}

// Steps in rail order: by depth, then as planned.
export function railOrder(steps: StepView[]): { step: StepView; layer: number }[] {
  const layers = stepLayers(steps);
  return steps
    .map((step, i) => ({ step, layer: layers.get(step.key) ?? 0, i }))
    .sort((a, b) => a.layer - b.layer || a.i - b.i)
    .map(({ step, layer }) => ({ step, layer }));
}

// A connector in the vertical rail: the column it runs down, and whether
// the need it comes from is met (done or skipped).
export interface RailLine {
  col: number;
  met: boolean;
}

export interface RailRow {
  step: StepView;
  layer: number;
  // The column of this step's dot.
  col: number;
  // Lines that end at this dot, from the column they came down.
  into: RailLine[];
  // Lines that leave this dot for a step further down.
  out: RailLine[];
  // Lines passing this row on their way down.
  through: RailLine[];
}

export interface RailLayout {
  rows: RailRow[];
  cols: number;
}

const isMet = (s: StepStatus) => s === "done" || s === "skipped";

// The vertical rail, drawn like a commit graph: every need is a line from
// the need's dot down to the step's. A step sits in the column of a line
// that ends at it; its first dependent continues down the same column and
// the others branch into free columns.
export function railLayout(steps: StepView[]): RailLayout {
  const order = railOrder(steps);
  const rowOf = new Map(order.map((o, i) => [o.step.key, i]));
  const children = new Map<string, string[]>();
  for (const { step } of order) {
    const to = rowOf.get(step.key) ?? -1;
    for (const n of new Set(step.needs)) {
      const from = rowOf.get(n);
      if (from === undefined || from >= to) continue;
      const list = children.get(n) ?? [];
      list.push(step.key);
      children.set(n, list);
    }
  }
  for (const list of children.values()) list.sort((a, b) => (rowOf.get(a) ?? 0) - (rowOf.get(b) ?? 0));
  const lanes: ({ to: string; met: boolean } | null)[] = [];
  const free = () => {
    const at = lanes.indexOf(null);
    return at < 0 ? lanes.length : at;
  };
  const rows: RailRow[] = [];
  let cols = 1;
  for (const { step, layer } of order) {
    const into: RailLine[] = [];
    lanes.forEach((l, c) => {
      if (l && l.to === step.key) into.push({ col: c, met: l.met });
    });
    for (const l of into) lanes[l.col] = null;
    const col = into.length ? Math.min(...into.map((l) => l.col)) : free();
    const through: RailLine[] = [];
    lanes.forEach((l, c) => {
      if (l) through.push({ col: c, met: l.met });
    });
    const met = isMet(step.status);
    const out: RailLine[] = [];
    (children.get(step.key) ?? []).forEach((k, i) => {
      const c = i === 0 ? col : free();
      lanes[c] = { to: k, met };
      out.push({ col: c, met });
    });
    cols = Math.max(cols, col + 1, lanes.length);
    rows.push({ step, layer, col, into, out, through });
  }
  return { rows, cols };
}

export const RAIL_MAX = 12;

// The compact rail: dots in rail order, at most `max`. `next` says how a
// dot joins the one before: a line into a later stage, or a small gap
// between steps that run side by side.
export function compactRail(
  steps: StepView[],
  max = RAIL_MAX,
): { dots: { key: string; title: string; status: StepStatus; next: "line" | "side" | "" }[]; more: number } {
  const order = railOrder(steps);
  const shown = order.slice(0, max);
  return {
    dots: shown.map(({ step, layer }, i) => ({
      key: step.key,
      title: step.title,
      status: step.status,
      next: i === 0 ? "" : layer === shown[i - 1].layer ? "side" : "line",
    })),
    more: Math.max(0, order.length - max),
  };
}

// How far the plan is: steps done (or skipped), and the step being worked
// now (else the next one to look at), with its place in the rail.
export function stepProgress(steps: StepView[]): { done: number; total: number; current: StepView | null; index: number } {
  const order = railOrder(steps).map((o) => o.step);
  const done = order.filter((s) => isMet(s.status)).length;
  let pick = order.findIndex((s) => s.status === "running");
  if (pick < 0) pick = order.findIndex((s) => s.status === "check" || s.status === "failed");
  if (pick < 0) pick = order.findIndex((s) => s.status === "todo");
  return { done, total: order.length, current: pick >= 0 ? order[pick] : null, index: pick + 1 };
}

// Where a session sits in the work: the goal it leads or the step it works.
export interface SessionPlace {
  goal: GoalView;
  step: StepView | null;
  root: boolean;
  // The step's place in the rail (1-based), 0 when there is none.
  index: number;
  total: number;
}

export function placeOf(goals: GoalView[], session: string | null | undefined): SessionPlace | null {
  if (!session) return null;
  for (const g of goals) {
    if (g.root === session) {
      const p = stepProgress(g.steps);
      return { goal: g, step: p.current, root: true, index: p.index, total: p.total };
    }
    const order = railOrder(g.steps);
    const at = order.findIndex((o) => o.step.sessions.includes(session));
    if (at >= 0) return { goal: g, step: order[at].step, root: false, index: at + 1, total: order.length };
  }
  // A helper spawned without a step still belongs to its goal while it runs.
  for (const g of goals) if (g.running.includes(session)) return { goal: g, step: null, root: false, index: 0, total: g.steps.length };
  return null;
}

export function basename(path: string): string {
  const parts = path.replace(/[\\/]+$/, "").split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

// A tool label as one short line: paths read as their file name
// ("Editing D:\\src\\Omnibar.svelte" is "Editing Omnibar.svelte").
export function shortLabel(label: string): string {
  return label
    .replace(/`/g, "")
    .replace(/\s+/g, " ")
    .trim()
    .split(" ")
    .map((w) => (/^[a-z]+:\/\//i.test(w) || !/[\\/]/.test(w) || w.length < 3 ? w : basename(w)))
    .join(" ");
}

// At most `max` characters, cut at a word where it can be.
export function clip(text: string, max: number): string {
  if (text.length <= max) return text;
  const cut = text.slice(0, max - 1);
  const space = cut.lastIndexOf(" ");
  return `${(space > max * 0.6 ? cut.slice(0, space) : cut).trimEnd()}…`;
}

export interface Activity {
  label: string;
  at: number;
}

// What the goal's agents are doing right now: the newest tool call among its
// running sessions, and how many other agents are on it.
export function goalActivity(g: Pick<GoalView, "running">, act: ReadonlyMap<string, Activity>): { text: string; more: number } | null {
  let latest: Activity | null = null;
  for (const s of g.running) {
    const a = act.get(s);
    if (a && (!latest || a.at > latest.at)) latest = a;
  }
  return latest ? { text: shortLabel(latest.label), more: g.running.length - 1 } : null;
}

// A card's second line: what its agents are doing right now, else why it
// needs you, the step being worked, or what the last run said.
export function cardLine(g: GoalView, act: ReadonlyMap<string, Activity>): { text: string; live: boolean; more: number } {
  const quiet = (text: string) => ({ text, live: false, more: 0 });
  if (g.status === "working") {
    const a = goalActivity(g, act);
    if (a) return { ...a, live: true };
  }
  const last = g.last;
  if (g.status === "check") {
    const bad = (last?.checks ?? []).filter((c) => !c.ok);
    if (bad.length) return quiet(bad.length > 1 ? `${bad[0].cmd} and ${bad.length - 1} more failed` : `${bad[0].cmd} failed`);
  }
  if (g.status === "failed" && last?.error) return quiet(firstLine(last.error));
  if (g.status === "paused") return quiet(firstLine(last?.summary) || "Reached its budget");
  if (g.status === "working") {
    const p = stepProgress(g.steps);
    if (p.current) return quiet(`Step ${p.index} of ${p.total}: ${p.current.title}`);
  }
  if (g.status === "later") return quiet(firstLine(g.prompt));
  return quiet(firstLine(last?.summary) || firstLine(g.prompt));
}

// The first line of a run's summary, without markdown marks.
export function firstLine(text: string | null | undefined): string {
  for (const raw of (text ?? "").split("\n")) {
    const line = raw
      .replace(/^\s*(#+|[-*>]|\d+\.)\s+/, "")
      .replace(/[*_`]/g, "")
      .trim();
    if (line) return line;
  }
  return "";
}

export function formatElapsed(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return m % 60 ? `${h}h ${m % 60}m` : `${h}h`;
  const d = Math.floor(h / 24);
  return h % 24 ? `${d}d ${h % 24}h` : `${d}d`;
}

export function formatCost(usd: number | null | undefined): string {
  if (usd === null || usd === undefined || !Number.isFinite(usd) || usd <= 0) return "";
  if (usd < 0.01) return "<$0.01";
  if (usd < 100) return `$${usd.toFixed(2)}`;
  return `$${Math.round(usd)}`;
}

export function ago(at: number, now = Date.now()): string {
  const s = (now - at) / 1000;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

// The card's time: how long the open run has been going, when it finished,
// or when it was queued.
export function timeOf(g: GoalView, now = Date.now()): string {
  if (g.status === "working" && g.last && g.last.ended === null) return formatElapsed(now - g.last.started);
  if (g.status === "later") return `Added ${ago(g.created, now)}`;
  return ago(g.updated, now);
}

const RUN_WORDS: Record<NonNullable<RunView["end"]>, string> = {
  completed: "Finished",
  failed: "Failed",
  paused: "Paused: budget",
  stopped: "Stopped",
  crashed: "Crashed",
  interrupted: "Cut off",
  not_started: "Didn't start",
};

export function runWord(r: Pick<RunView, "end" | "ended">): string {
  return r.end ? RUN_WORDS[r.end] : r.ended === null ? "Running" : "Ended";
}

export function runTone(r: Pick<RunView, "end" | "ended" | "checks">): Tone {
  if (!r.end) return r.ended === null ? "live" : "idle";
  if (r.end === "completed") return r.checks.some((c) => !c.ok) ? "need" : "ok";
  if (r.end === "paused") return "need";
  if (r.end === "failed" || r.end === "crashed" || r.end === "not_started") return "bad";
  return "idle";
}

// How long a run took, or has been going.
export function runTime(r: Pick<RunView, "ended" | "started" | "ms">, now = Date.now()): string {
  return formatElapsed(r.ended === null ? now - r.started : r.ms || (r.ended ?? r.started) - r.started);
}

// A time of day today, else the date too.
export function whenOf(at: number, now = Date.now()): string {
  const d = new Date(at);
  const time = d.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  return at >= startOfDay(now) ? time : `${d.toLocaleDateString(undefined, { month: "short", day: "numeric" })}, ${time}`;
}

export function engineLabel(e: Engine): string {
  if (e.kind === "here") return e.name || "This PC";
  if (e.kind === "server") return e.name && e.name !== "Server" ? `Server (${e.name})` : "Server";
  return e.name || "Another device";
}

// The engine filter shows only once work comes from somewhere else.
export function otherEngines(goals: GoalView[], serverLinked = false): boolean {
  return serverLinked || goals.some((g) => g.engine.kind !== "here");
}

export type EngineFilter = "all" | "here" | "server";

export function engineMatch(g: GoalView, f: EngineFilter): boolean {
  return f === "all" || g.engine.kind === f;
}

// The project filter value for every project; "" is work without one.
export const ALL = "*";

export function sameProject(a: string | null, b: string | null): boolean {
  return (a ?? "").toLowerCase() === (b ?? "").toLowerCase();
}

export function inProject(g: GoalView, scope: string): boolean {
  return scope === ALL || sameProject(g.project, scope || null);
}

export interface ProjectOption {
  value: string;
  label: string;
  count: number;
}

// All projects, then each project with work (open work counted), then work
// without a project.
export function projectOptions(goals: GoalView[], projects: { slug: string; title: string }[], now = Date.now()): ProjectOption[] {
  const listed = goals.filter((g) => bandOf(g, [], now) !== null);
  const open = (g: GoalView) => {
    const b = bandOf(g, [], now);
    return b === "needs" || b === "working" || b === "later";
  };
  const titles = new Map(projects.map((p) => [p.slug.toLowerCase(), p.title]));
  const slugs = new Map<string, string>();
  for (const g of listed) if (g.project) slugs.set(g.project.toLowerCase(), g.project);
  const rows = [...slugs.values()]
    .map((slug) => ({ value: slug, label: titles.get(slug.toLowerCase()) ?? slug, count: listed.filter((g) => open(g) && sameProject(g.project, slug)).length }))
    .sort((a, b) => a.label.localeCompare(b.label));
  const loose = listed.filter((g) => g.project === null);
  return [
    { value: ALL, label: "All projects", count: listed.filter(open).length },
    ...rows,
    ...(loose.length ? [{ value: "", label: "No project", count: loose.filter(open).length }] : []),
  ];
}

// Home's card: what needs you, what is being worked, what finished today.
export function homeSummary(
  goals: GoalView[],
  asks: AskView[] = [],
  now = Date.now(),
  dayStart = startOfDay(now),
): { needs: GoalView[]; working: GoalView[]; doneToday: GoalView[] } {
  const b = groupBands(goals, asks, now);
  return { needs: b.needs, working: b.working, doneToday: b.done.filter((g) => g.status === "done" && g.updated >= dayStart) };
}

export function startOfDay(now: number): number {
  const d = new Date(now);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}
