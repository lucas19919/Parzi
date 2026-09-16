/**
 * Deck state that needs the host: the drafts store the right inspector
 * reads, and the one lookup the legacy mount needs. The derivations live
 * in `derive.ts` (pure, unit-tested) and are re-exported here so the deck
 * components have one import.
 */
import { invoke } from "@tauri-apps/api/core";
import { writable } from "svelte/store";
import { deck, type AuditResult, type Draft, type Plan, type Project } from "../api";
import type { TaskLive } from "./derive";

export {
  allTasks,
  etagOf,
  laneNames,
  liveLaneCount,
  projectMarkdown,
  restingState,
  sprintPosition,
  taskStates,
} from "./derive";
export type { TaskLive, TaskState } from "./derive";

/**
 * A `critical` lease transfer waiting for a person. It arrives as the same
 * `approval` run event a tool approval does, so the card is the same card.
 */
export interface DeckApproval {
  key: string;
  task: string;
  lane: string;
  name: string;
  detail: string;
}

/**
 * The open project as the right panel's Project tab sees it. The deck owns
 * the data and the poll; the panel only reads this and calls the actions.
 * `null` while no project is open, which also hides the tab.
 */
export interface ProjectPanelState {
  project: Project;
  plan: Plan;
  live: Record<string, TaskLive>;
  approvals: Record<string, DeckApproval>;
  drafts: Draft[];
  audit: AuditResult | null;
  auditing: string;
  approving: boolean;
  actions: {
    audit: (draft: Draft) => void;
    approve: () => void;
    answer: (key: string, allow: boolean) => void;
    save: (project: Project) => void;
    openDraft: (draft: Draft) => void;
  };
}

export const projectPanel = writable<ProjectPanelState | null>(null);

/**
 * Drafts of the open project, published for the right deck's Docs tab
 * (DocReader lists them as quick tabs). Empty when no project is open.
 */
export const deckDrafts = writable<{ label: string; path: string }[]>([]);

/** Legacy mount (ProjectMainPage) knows a slug only: find its workspace. */
export async function resolveWorkspace(slug: string): Promise<string> {
  const names = await invoke<string[]>("workspace_list");
  for (const ws of names) {
    try {
      const projects = await deck.list(ws);
      if (projects.some((p) => p.slug === slug)) return ws;
    } catch {
      // A workspace that fails to list is not this project's workspace.
    }
  }
  return names[0] ?? "";
}
