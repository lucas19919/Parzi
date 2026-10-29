import type { Tab } from "./tabs";

export interface DeskTab {
  id: string;
  kind: "browser" | "session" | "settings" | "preview" | "projects" | "brain" | "history" | "artifact";
  title: string;
  url: string;
  session_id: string;
  cwd: string;
}

export interface DeskCmd {
  op: "open" | "focus" | "close" | "navigate" | "show" | "notify" | "approval" | "deleted";
  id?: string;
  url?: string;
  rev?: number;
  owner?: string | null;
  focus?: boolean;
  what?: "session" | "settings" | "projects" | "brain" | "history" | "plan" | "work" | "tasks" | "home";
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





export function deskRows(tabs: Tab[]): DeskTab[] {
  return tabs.map((t) => ({
    id: t.id,
    kind: t.kind === "page" ? "browser" : (t.kind as DeskTab["kind"]),
    title: t.title,
    url: t.url ?? "",
    session_id: (t.kind === "page" ? t.owner : t.sessionId) ?? "",
    cwd: t.cwd ?? "",
  }));
}
