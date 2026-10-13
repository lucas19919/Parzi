// The plan panel's rules, pure so tests pin them down: what state each item
// is in right now (read from what agents declared, the sessions linked to
// it, their runs and the PRs on its branches), how items group, and how
// their needs are drawn. Agents only declare planned, active, done or
// dropped; everything else is derived here and never stored.
import type { Plan, PlanItem, PlanQuote, PrInfo, RunOutcome, SessionMeta } from "./api";
import type { Activity } from "./activity";

export type ItemState = "done" | "dropped" | "needs" | "working" | "review" | "started" | "todo";

export const STATE_WORDS: Record<ItemState, string> = {
  done: "Done",
  dropped: "Dropped",
  needs: "Needs you",
  working: "Working",
  review: "In review",
  started: "Started",
  todo: "Not started",
};

export function sameSlug(a: string | null | undefined, b: string | null | undefined): boolean {
  return (a ?? "").toLowerCase() === (b ?? "").toLowerCase();
}

// Branches that say nothing about an item.
export function isTrunk(branch: string): boolean {
  return ["", "main", "master", "head"].includes(branch.trim().toLowerCase());
}

export function isRunning(s: SessionMeta, running: ReadonlySet<string>): boolean {
  return running.has(s.id) || s.status === "active";
}

// Each item's sessions: the ones in this project that name it, and what
// they spawned (a child that names another item counts for that one).
export function linkedByItem(threads: SessionMeta[], project: string): Map<string, SessionMeta[]> {
  const kids = new Map<string, SessionMeta[]>();
  for (const t of threads) {
    if (!t.parent_id) continue;
    const list = kids.get(t.parent_id) ?? [];
    list.push(t);
    kids.set(t.parent_id, list);
  }
  const out = new Map<string, SessionMeta[]>();
  for (const t of threads) {
    const item = t.plan_item;
    if (!item || !sameSlug(t.project, project)) continue;
    const list = out.get(item) ?? [];
    const seen = new Set(list.map((s) => s.id));
    const walk = (s: SessionMeta) => {
      if (seen.has(s.id)) return;
      seen.add(s.id);
      list.push(s);
      for (const c of kids.get(s.id) ?? []) if (!c.plan_item || c.plan_item === item) walk(c);
    };
    walk(t);
    out.set(item, list);
  }
  return out;
}

// Why a finished run needs you, or "".
export function runTrouble(run: RunOutcome | null | undefined): string {
  if (!run) return "";
  if (run.end === "failed") return "The last run failed";
  if (run.end === "crashed") return "The last run crashed";
  if (run.end === "paused") return "Paused at its budget";
  const bad = (run.checks ?? []).filter((c) => !c.ok);
  if (!bad.length) return "";
  return bad.length > 1 ? `${bad[0].cmd} and ${bad.length - 1} more failed` : `${bad[0].cmd} failed`;
}

export interface StateFacts {
  // A PR on one of its branches merged.
  merged: boolean;
  // A linked session waits on an approval or a question.
  asks: boolean;
  // Nothing runs and the newest finished run failed, paused or failed a check.
  trouble: boolean;
  // A linked session runs.
  working: boolean;
  // An open PR on one of its branches.
  review: boolean;
  // It has sessions or a branch.
  touched: boolean;
}

// The precedence, top first.
export function itemState(item: Pick<PlanItem, "state">, f: StateFacts): ItemState {
  if (item.state === "done" || f.merged) return "done";
  if (item.state === "dropped") return "dropped";
  if (f.asks || f.trouble) return "needs";
  if (f.working) return "working";
  if (f.review) return "review";
  if (item.state === "active" || f.touched) return "started";
  return "todo";
}

export interface PlanInputs {
  project: string;
  threads: SessionMeta[];
  running: ReadonlySet<string>;
  // Session -> what it waits on you for (an approval or a question).
  waiting: ReadonlyMap<string, string>;
  prs: PrInfo[];
  activity: ReadonlyMap<string, Activity>;
}

export interface ItemView {
  item: PlanItem;
  state: ItemState;
  sessions: SessionMeta[];
  // Linked sessions running now.
  agents: number;
  branches: string[];
  // The open PR on its branches, else the merged one.
  pr: PrInfo | null;
  // The newest finished run among its sessions.
  last: { session: SessionMeta; run: RunOutcome } | null;
  // Why it needs you ("" unless it does).
  reason: string;
  // What its agents do right now: the newest tool call's label.
  doing: string;
  // Needs not met yet, by title.
  after: string[];
}

function newestPr(list: PrInfo[]): PrInfo | null {
  return list.reduce<PrInfo | null>((best, p) => (!best || p.number > best.number ? p : best), null);
}

export function newestRun(sessions: SessionMeta[]): { session: SessionMeta; run: RunOutcome } | null {
  let best: { session: SessionMeta; run: RunOutcome } | null = null;
  for (const s of sessions) {
    const run = s.last_run;
    if (run && (!best || run.ended > best.run.ended)) best = { session: s, run };
  }
  return best;
}

// The newest tool call among these sessions (its raw label), or "".
export function newestActivity(ids: string[], act: ReadonlyMap<string, Activity>): string {
  let latest: Activity | null = null;
  for (const id of ids) {
    const a = act.get(id);
    if (a && (!latest || a.at > latest.at)) latest = a;
  }
  return latest?.label ?? "";
}

export function deriveItems(plan: Plan, inp: PlanInputs): ItemView[] {
  const linked = linkedByItem(inp.threads, inp.project);
  const views: ItemView[] = plan.items.map((item) => {
    const sessions = linked.get(item.id) ?? [];
    const live = sessions.filter((s) => isRunning(s, inp.running));
    const branches = [...new Set([item.branch, ...sessions.map((s) => s.branch ?? "")].map((b) => b.trim()))].filter((b) => !isTrunk(b));
    const prs = inp.prs.filter((p) => branches.includes(p.head));
    const open = newestPr(prs.filter((p) => p.state === "open"));
    const merged = newestPr(prs.filter((p) => p.state === "merged"));
    const ask = sessions.map((s) => inp.waiting.get(s.id) ?? "").find(Boolean) ?? "";
    // A run going now supersedes how the last one ended.
    const trouble = live.length ? "" : runTrouble(newestRun(sessions)?.run);
    const state = itemState(item, {
      merged: !!merged,
      asks: !!ask,
      trouble: !!trouble,
      working: live.length > 0,
      review: !!open,
      touched: sessions.length > 0 || branches.length > 0,
    });
    return {
      item,
      state,
      sessions,
      agents: live.length,
      branches,
      pr: open ?? merged,
      last: newestRun(sessions),
      reason: state === "needs" ? ask || trouble : "",
      doing: state === "working" ? newestActivity(live.map((s) => s.id), inp.activity) : "",
      after: [],
    };
  });
  const byId = new Map(views.map((v) => [v.item.id, v]));
  for (const v of views) {
    v.after = v.item.needs
      .filter((n) => n !== v.item.id)
      .map((n) => byId.get(n))
      .filter((n): n is ItemView => !!n && n.state !== "done" && n.state !== "dropped")
      .map((n) => n.item.title);
  }
  return views;
}

export function isClosed(state: ItemState): boolean {
  return state === "done" || state === "dropped";
}

export function progress(views: ItemView[]): { done: number; total: number } {
  return { done: views.filter((v) => v.state === "done").length, total: views.filter((v) => v.state !== "dropped").length };
}

export interface PhaseGroup {
  // "" when the plan has no phases.
  phase: string;
  items: ItemView[];
}

// Open items by phase, in the plan's order (an item without a phase is in
// the first one), and the closed ones apart.
export function groupItems(plan: Plan, views: ItemView[]): { open: PhaseGroup[]; closed: ItemView[] } {
  const first = plan.phases[0] ?? "";
  const groups = new Map<string, ItemView[]>((plan.phases.length ? plan.phases : [""]).map((p) => [p, []]));
  const closed: ItemView[] = [];
  for (const v of views) {
    if (isClosed(v.state)) {
      closed.push(v);
      continue;
    }
    const phase = v.item.phase || first;
    const list = groups.get(phase) ?? [];
    list.push(v);
    groups.set(phase, list);
  }
  return { open: [...groups].filter(([, items]) => items.length).map(([phase, items]) => ({ phase, items })), closed };
}

// The rail beside the items: a spine through every node, and each open
// need drawn as a connector from the need's node, out to a lane on the
// left, down, and into the node that waits on it (one lane per need, shared
// by its dependents; lanes are reused once free, like a commit graph).
export interface RailRow {
  // Lanes passing this row without touching its node.
  through: number[];
  // Lanes that join this node: `ends` when it is the lane's last stop.
  into: { lane: number; ends: boolean }[];
  // The lane this node's connector leaves on, or -1.
  out: number;
  // The spine reaches the row above / below.
  up: boolean;
  down: boolean;
}

export interface RailLayout {
  rows: RailRow[];
  lanes: number;
}

// `rows`: item ids in display order, null for rows without a node (a phase
// heading). Needs that point up the list are left out (the item still says
// "after"), and connectors past `maxLanes` too.
export function railLayout(rows: (string | null)[], needsOf: ReadonlyMap<string, string[]>, maxLanes = 4): RailLayout {
  const rowOf = new Map<string, number>();
  rows.forEach((id, i) => {
    if (id !== null && !rowOf.has(id)) rowOf.set(id, i);
  });
  const deps = new Map<string, number[]>();
  rows.forEach((id, j) => {
    if (id === null) return;
    for (const n of new Set(needsOf.get(id) ?? [])) {
      const i = rowOf.get(n);
      if (i === undefined || i >= j) continue;
      const list = deps.get(n) ?? [];
      list.push(j);
      deps.set(n, list);
    }
  });
  const sources = [...deps].map(([id, list]) => ({ from: rowOf.get(id) ?? 0, to: Math.max(...list), stops: new Set(list) })).sort((a, b) => a.from - b.from);
  const laneEnd: number[] = [];
  const placed: { lane: number; from: number; to: number; stops: Set<number> }[] = [];
  for (const s of sources) {
    let lane = laneEnd.findIndex((end) => end < s.from);
    if (lane < 0) lane = laneEnd.length;
    if (lane >= maxLanes) continue;
    laneEnd[lane] = s.to;
    placed.push({ lane, ...s });
  }
  const nodes = rows.map((id, i) => (id === null ? -1 : i)).filter((i) => i >= 0);
  const firstNode = nodes[0] ?? -1;
  const lastNode = nodes[nodes.length - 1] ?? -1;
  const out: RailRow[] = rows.map((id, r) => {
    const node = id !== null;
    const row: RailRow = {
      through: [],
      into: [],
      out: -1,
      up: firstNode >= 0 && (node ? r > firstNode : r > firstNode && r < lastNode),
      down: lastNode >= 0 && (node ? r < lastNode : r > firstNode && r < lastNode),
    };
    for (const p of placed) {
      if (r === p.from) row.out = p.lane;
      else if (node && p.stops.has(r)) row.into.push({ lane: p.lane, ends: r === p.to });
      else if (r > p.from && r < p.to) row.through.push(p.lane);
    }
    return row;
  });
  return { rows: out, lanes: laneEnd.length };
}

// A session's dot: running, waiting on you, failed, done, or quiet.
export type Tone = "live" | "need" | "bad" | "ok" | "idle";

export const TONE_WORDS: Record<Tone, string> = { live: "Running", need: "Needs you", bad: "Failed", ok: "Done", idle: "Idle" };

export function sessionTone(s: SessionMeta, running: ReadonlySet<string>, waiting?: { has(id: string): boolean }): Tone {
  if (waiting?.has(s.id)) return "need";
  if (isRunning(s, running)) return "live";
  const r = s.last_run;
  if (!r) return "idle";
  if (r.end === "failed" || r.end === "crashed" || r.end === "not_started") return "bad";
  if (r.end === "paused" || (r.checks ?? []).some((c) => !c.ok)) return "need";
  return r.end === "completed" ? "ok" : "idle";
}

// This project's sessions outside the plan, while they run or wait on you:
// one row per tree (a helper shows under the session that spawned it).
export function alsoRunning(
  threads: SessionMeta[],
  project: string,
  linked: ReadonlySet<string>,
  running: ReadonlySet<string>,
  waiting: { has(id: string): boolean },
): { session: SessionMeta; ids: string[] }[] {
  const byId = new Map(threads.map((t) => [t.id, t]));
  const busy = threads.filter((t) => sameSlug(t.project, project) && !linked.has(t.id) && (isRunning(t, running) || waiting.has(t.id)));
  const busyIds = new Set(busy.map((t) => t.id));
  const rootOf = (t: SessionMeta) => {
    let root = t;
    let cur = t.parent_id ? byId.get(t.parent_id) : undefined;
    for (let guard = 0; cur && guard < 50; guard++) {
      if (busyIds.has(cur.id)) root = cur;
      cur = cur.parent_id ? byId.get(cur.parent_id) : undefined;
    }
    return root;
  };
  const rows = new Map<string, { session: SessionMeta; ids: string[] }>();
  for (const t of busy) {
    const root = rootOf(t);
    const row = rows.get(root.id) ?? { session: root, ids: [] };
    row.ids.push(t.id);
    rows.set(root.id, row);
  }
  return [...rows.values()].sort((a, b) => (Date.parse(b.session.updated) || 0) - (Date.parse(a.session.updated) || 0));
}

// Quotes travel as plain prompt text (PLAN_CONTRACT.md):
//   <plan-item project="parzi" id="names" title="Session names">
//   ...
//   </plan-item>
const BLOCK = /\n*<plan-item\b([^>]*)>[\s\S]*?<\/plan-item>\n*/g;

function unescapeAttr(v: string): string {
  return v.replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
}

function escapeAttr(v: string): string {
  return v.replace(/&/g, "&amp;").replace(/"/g, "&quot;");
}

// A user message without its quote blocks, and what they quoted.
export function splitQuotes(text: string): { text: string; quotes: PlanQuote[] } {
  const quotes: PlanQuote[] = [];
  const rest = text.replace(BLOCK, (_, attrs: string) => {
    const a: Record<string, string> = {};
    for (const m of attrs.matchAll(/([a-z_]+)="([^"]*)"/g)) a[m[1]] = unescapeAttr(m[2]);
    quotes.push({ project: a.project ?? "", id: a.id ?? "", title: a.title || a.id || "Plan item" });
    return "\n\n";
  });
  return { text: quotes.length ? rest.trim() : text, quotes };
}

// The block the desk appends for a quote, as the thread shows it until the
// sent message comes back.
export function quoteBlock(q: PlanQuote): string {
  return `<plan-item project="${escapeAttr(q.project)}" id="${escapeAttr(q.id)}" title="${escapeAttr(q.title)}">\n</plan-item>`;
}

export const MAX_QUOTES = 4;

// The composer's quotes with one more: once each, and never past four.
export function addQuote(list: PlanQuote[], q: PlanQuote): PlanQuote[] {
  if (list.length >= MAX_QUOTES || list.some((x) => x.id === q.id && sameSlug(x.project, q.project))) return list;
  return [...list, q];
}

export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

// Where an item's hover card goes: beside its row, level with it (the panel
// sits at the window's right edge, so left first), else below or above it;
// always inside the window.
export function cardSpot(anchor: Box, card: { width: number; height: number }, view: { width: number; height: number }, gap = 8): { left: number; top: number } {
  const top = Math.max(gap, Math.min(anchor.top, view.height - card.height - gap));
  if (anchor.left - gap - card.width >= gap) return { left: Math.round(anchor.left - gap - card.width), top: Math.round(top) };
  if (anchor.right + gap + card.width <= view.width - gap) return { left: Math.round(anchor.right + gap), top: Math.round(top) };
  const left = Math.max(gap, Math.min(anchor.left, view.width - card.width - gap));
  const below = anchor.bottom + gap;
  const y = below + card.height <= view.height - gap ? below : Math.max(gap, anchor.top - gap - card.height);
  return { left: Math.round(left), top: Math.round(y) };
}
