// The plan and PRs of the session on screen, loaded once and kept fresh:
// the inline checklist and the info column read the same copy.
import { writable, get } from "svelte/store";
import { onPlan, onRunEvent, planApi, type Plan, type PrInfo } from "./api";
import { sameSlug } from "./planModel";
import { coalesce } from "./threadList";

export interface PlanState {
  project: string;
  plan: Plan | null;
  // The plan note exists but can't be read.
  error: string;
  // PRs in the folder (`gh`): empty without it.
  prs: PrInfo[];
}

export const planState = writable<PlanState>({ project: "", plan: null, error: "", prs: [] });

const PR_EVERY = 120_000;

let project = "";
let folder = "";
let listening = false;

const loadPlan = coalesce(async () => {
  const slug = project;
  if (!slug) return;
  try {
    const view = await planApi.view(slug);
    if (slug === project) planState.update((s) => ({ ...s, plan: view.plan, error: view.error ?? "" }));
  } catch {
    // A failed load keeps what was shown; the next change or focus retries.
  }
}, 150);

const loadPrs = coalesce(async () => {
  const dir = folder;
  if (!dir) return;
  const prs = await planApi.prs(dir).catch(() => [] as PrInfo[]);
  if (dir === folder) planState.update((s) => ({ ...s, prs }));
}, 500);

function listen() {
  if (listening) return;
  listening = true;
  void onPlan((e) => {
    if (sameSlug(e.project, project)) void loadPlan();
  });
  void onRunEvent((e) => {
    if (e.kind !== "done" && e.kind !== "error" && e.kind !== "paused") return;
    void loadPlan();
    void loadPrs();
  });
  window.addEventListener("focus", () => {
    void loadPlan();
    void loadPrs();
  });
  setInterval(() => {
    if (!document.hidden) void loadPrs();
  }, PR_EVERY);
}

// Points the store at a project and folder; the same pair again is free.
export function watchPlan(slug: string, dir: string) {
  listen();
  if (slug !== project) {
    project = slug;
    planState.set({ ...get(planState), project: slug, plan: null, error: "" });
    void loadPlan();
  }
  if (dir !== folder) {
    folder = dir;
    planState.update((s) => ({ ...s, prs: [] }));
    void loadPrs();
  }
}
