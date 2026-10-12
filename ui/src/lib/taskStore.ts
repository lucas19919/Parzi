// The shared task list, live: one copy for the Tasks view, the side panel
// and Home, refreshed whenever the backend says tasks changed.
import { writable } from "svelte/store";
import { api, onTasks, taskApi, type Task } from "./api";
import { persisted } from "./browserData";
import { permissionFor } from "./lanes";
import { coalesce } from "./threadList";
import { chime, notifyAction, toast } from "./toast";

export const tasks = writable<Task[]>([]);
export const tasksLoaded = writable(false);
export const tasksError = writable("");

export const refreshTasks = coalesce(async () => {
  try {
    tasks.set(await taskApi.list());
    tasksError.set("");
  } catch (e) {
    tasksError.set(String(e));
  }
  tasksLoaded.set(true);
}, 80);

// Opening the Tasks view, at a task: App watches the counter (like Brain).
export const targetTask = writable("");
export const taskTabRequested = writable(0);

export function openTask(id = "") {
  targetTask.set(id);
  taskTabRequested.update((n) => n + 1);
}

function short(title: string) {
  return title.length > 48 ? `${title.slice(0, 47)}…` : title;
}

const FINISHED: Record<string, string> = {
  review: "is ready for review",
  done: "is done",
  failed: "failed",
};

// Live updates, and a toast (click to open) when a run hands a task back.
export function watchTasks(): () => void {
  void refreshTasks();
  const un = onTasks((c) => {
    void refreshTasks();
    for (const t of c.finished) {
      const what = FINISHED[t.status];
      if (!what) continue;
      notifyAction(`Task "${short(t.title)}" ${what}`, () => openTask(t.id), t.status === "failed");
      chime(t.status !== "failed");
    }
  });
  return () => {
    un.then((f) => f()).catch(() => {});
  };
}

// The assign form remembers its last choice on this device. An empty mode
// means "the Settings policy".
export interface AssignPrefs {
  model: string;
  effort: string;
  mode: string;
}

export const assignPrefs = persisted<AssignPrefs>("parzi.tasks.assign.v1", { model: "", effort: "medium", mode: "" });

export async function policyMode(): Promise<string> {
  const cfg = await api.getConfig().catch(() => null);
  return permissionFor(cfg?.lanes?.default_mode);
}

// Assign with the remembered model and mode; false when there is no model yet.
export async function quickAssign(t: Task, prefs: AssignPrefs): Promise<boolean> {
  if (!prefs.model) return false;
  const mode = prefs.mode || (await policyMode());
  try {
    const r = await taskApi.assign({ id: t.id, model: prefs.model, effort: prefs.effort || "medium", mode });
    toast(r.warning ? `Agent started. ${r.warning}` : "Agent started on the task");
  } catch (e) {
    toast(String(e), true);
  }
  return true;
}
