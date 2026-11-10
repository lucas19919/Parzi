import { writable, get } from "svelte/store";
import { onPlan, planApi, type GoalsView } from "./api";
import { sameSlug } from "./planModel";
import { coalesce } from "./threadList";

export interface GoalsState {
  project: string;
  goals: GoalsView | null;
}

export const goalsState = writable<GoalsState>({ project: "", goals: null });

let project = "";
let listening = false;

const loadGoals = coalesce(async () => {
  const slug = project;
  if (!slug) return;
  try {
    const view = await planApi.goals(slug);
    if (slug === project) goalsState.update((s) => ({ ...s, goals: view }));
  } catch {}
}, 200);

function listen() {
  if (listening) return;
  listening = true;
  void onPlan((e) => {
    if (sameSlug(e.project, project)) void loadGoals();
  });
  window.addEventListener("focus", () => {
    void loadGoals();
  });
}

export function watchGoals(slug: string) {
  listen();
  if (slug !== project) {
    project = slug;
    goalsState.set({ ...get(goalsState), project: slug, goals: null });
    void loadGoals();
  }
}

/** A status word to a tone the card row can wear. */
export function statusTone(status: string): "ok" | "warn" | "bad" | "" {
  const s = status.trim().toLowerCase();
  if (!s) return "";
  if (["done", "complete", "completed", "shipped", "live"].includes(s)) return "ok";
  if (["active", "doing", "in_progress", "in-progress", "started", "working"].includes(s)) return "warn";
  if (["blocked", "dropped", "stalled", "failed"].includes(s)) return "bad";
  return "";
}
