import type { Tab } from "./tabs";

/** A tab as the window's control socket sees it (control.rs TabSnap). */
export interface DeskTab {
  id: string;
  kind: "browser" | "session" | "settings" | "preview" | "brain" | "history" | "tasks";
  title: string;
  url: string;
  /** The session a session tab shows, or the agent that owns a page tab. */
  session_id: string;
  /** A draft session tab's folder. */
  cwd: string;
}

/** What the control socket asks the window to do (`parzi://desk`). */
export interface DeskCmd {
  op: "open" | "focus" | "close" | "navigate" | "show" | "notify" | "approval" | "deleted";
  id?: string;
  url?: string;
  rev?: number;
  owner?: string | null;
  /** open, show: switch to it. Agents' pages open behind unless they ask. */
  focus?: boolean;
  what?: "session" | "settings" | "brain" | "history" | "tasks" | "home";
  title?: string;
  section?: string;
  path?: string;
  view?: "sessions" | "history" | "bookmarks" | "downloads";
  text?: string;
  session?: string;
  key?: string;
  allow?: boolean;
  label?: string;
}

/**
 * The rows the window syncs to the control socket. A page carries the
 * agent that owns it: without the owner an agent loses its tab after the
 * first sync.
 */
export function deskRows(tabs: Tab[]): DeskTab[] {
  return tabs.map((t) => ({
    id: t.id,
    kind: t.kind === "page" ? "browser" : t.kind,
    title: t.title,
    url: t.url ?? "",
    session_id: (t.kind === "page" ? t.owner : t.sessionId) ?? "",
    cwd: t.cwd ?? "",
  }));
}
