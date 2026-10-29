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

export function isTrunk(branch: string): boolean {
  return ["", "main", "master", "head"].includes(branch.trim().toLowerCase());
}

export function isRunning(s: SessionMeta, running: ReadonlySet<string>): boolean {
  return running.has(s.id) || s.status === "active";
}

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
  merged: boolean;
  asks: boolean;
  trouble: boolean;
  working: boolean;
  review: boolean;
  touched: boolean;
}

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
  waiting: ReadonlyMap<string, string>;
  prs: PrInfo[];
  activity: ReadonlyMap<string, Activity>;
}

export interface ItemView {
  item: PlanItem;
  state: ItemState;
  sessions: SessionMeta[];
  agents: number;
  branches: string[];
  pr: PrInfo | null;
  last: { session: SessionMeta; run: RunOutcome } | null;
  reason: string;
  doing: string;
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
  phase: string;
  items: ItemView[];
}

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

const BLOCK = /\n*<plan-item\b([^>]*)>[\s\S]*?<\/plan-item>\n*/g;

function unescapeAttr(v: string): string {
  return v.replace(/&quot;/g, '"').replace(/&#39;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");
}

function escapeAttr(v: string): string {
  return v.replace(/&/g, "&amp;").replace(/"/g, "&quot;");
}

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

export function quoteBlock(q: PlanQuote): string {
  return `<plan-item project="${escapeAttr(q.project)}" id="${escapeAttr(q.id)}" title="${escapeAttr(q.title)}">\n</plan-item>`;
}

export const MAX_QUOTES = 4;

export function addQuote(list: PlanQuote[], q: PlanQuote): PlanQuote[] {
  if (list.length >= MAX_QUOTES || list.some((x) => x.id === q.id && sameSlug(x.project, q.project))) return list;
  return [...list, q];
}
