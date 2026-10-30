import type { PlanQuote } from "./api";

export type ComposerMode = "search" | "build";
type TabKind = "session" | "page" | "history" | "settings" | "preview" | "artifact" | "manager";

export interface TabComposerState {
  input: string;
  mode: ComposerMode;
  model: string;
  effort: string;
  attachments: string[];
  quotes?: PlanQuote[];
  remote?: boolean;
  remoteCwd?: string;
  worktree?: string;
}

function defaultComposer(): TabComposerState {
  return ensureComposer();
}

const LAST_MODEL = "parzi.lastModel.v1";
const RECENT_MODELS = "parzi.recentModels.v1";

function lastModel(): { model: string; effort: string } {
  try {
    const v = JSON.parse(localStorage.getItem(LAST_MODEL) ?? "null");
    if (v && typeof v.model === "string") return { model: v.model, effort: typeof v.effort === "string" ? v.effort : "" };
  } catch {}
  return { model: "", effort: "" };
}

export function recentModels(): string[] {
  try {
    const v = JSON.parse(localStorage.getItem(RECENT_MODELS) ?? "[]");
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}

export function rememberModel(model: string, effort: string) {
  if (!model) return;
  try {
    localStorage.setItem(LAST_MODEL, JSON.stringify({ model, effort }));
    localStorage.setItem(RECENT_MODELS, JSON.stringify([model, ...recentModels().filter((m) => m !== model)].slice(0, 3)));
  } catch {}
}

export function ensureComposer(c?: Partial<TabComposerState> | null): TabComposerState {
  const full = c as TabComposerState | null | undefined;
  if (full && Array.isArray(full.attachments) && Array.isArray(full.quotes)) {
    if (!full.model) Object.assign(full, lastModel());
    return full;
  }
  const last = lastModel();
  const next: TabComposerState = {
    input: typeof c?.input === "string" ? c.input : "",
    mode: c?.mode === "search" ? "search" : "build",
    model: typeof c?.model === "string" && c.model ? c.model : last.model,
    effort: typeof c?.effort === "string" && c.effort ? c.effort : last.effort || "medium",
    remote: c?.remote,
    remoteCwd: c?.remoteCwd,
    attachments: Array.isArray(c?.attachments) ? c.attachments : [],
    quotes: Array.isArray(c?.quotes) ? c.quotes : [],
  };
  if (c) {
    Object.assign(c, next);
    return c as TabComposerState;
  }
  return next;
}

export interface Tab {
  id: string;
  kind: TabKind;
  title: string;
  sessionId?: string | null;
  url?: string;
  cwd?: string;
  owner?: string | null;
  group?: string | null;
  loading?: boolean;
  canGoBack?: boolean;
  canGoForward?: boolean;
  blocked?: number;
  bg?: string;
  favicon?: string;
  zoom?: number;
  crashed?: string;
  doc?: ArtifactDoc;
  composer: TabComposerState;
}

export interface ArtifactDoc {
  id: string;
  title: string;
  kind: string;
  language?: string;
  content: string;
  version: number;
}

let seq = 0;

function tabId(): string {
  seq += 1;
  return `tab-${Date.now().toString(36)}-${seq}`;
}

export function sessionTab(sessionId: string | null = null, title = "New session"): Tab {
  return { id: tabId(), kind: "session", title, sessionId, composer: defaultComposer() };
}

export function pageTab(url = "", id = tabId(), owner: string | null = null, group: string | null = null): Tab {
  return { id, kind: "page", title: hostOf(url) || "New page", url, owner, group, composer: defaultComposer() };
}

export function historyTab(): Tab {
  return { id: tabId(), kind: "history", title: "History", composer: defaultComposer() };
}

export function managerTab(): Tab {
  return { id: tabId(), kind: "manager", title: "Manager", composer: defaultComposer() };
}

const RESTORED = new Set(["session", "page", "history"]);

export function restoreTab<T extends { kind?: unknown; title?: unknown }>(t: T): T | null {
  return typeof t.kind === "string" && RESTORED.has(t.kind) ? t : null;
}

export function settingsTab(): Tab {
  return { id: tabId(), kind: "settings", title: "Settings", composer: defaultComposer() };
}

export function previewTab(which = "omnibar"): Tab {
  return { id: tabId(), kind: "preview", title: `Preview ${which}`, url: which, composer: defaultComposer() };
}

export function artifactTab(doc: ArtifactDoc, group: string | null): Tab {
  return { id: tabId(), kind: "artifact", title: doc.title, group, doc, composer: defaultComposer() };
}

export function hostOf(url: string): string {
  return url
    .replace(/^https?:\/\/(www\.)?/i, "")
    .replace(/[/?#].*$/, "")
    .slice(0, 32);
}

export function toAddress(raw: string): string {
  const t = raw.trim();
  if (!t) return "";
  if (/^https?:\/\//i.test(t)) return t;
  if (/^(localhost|127\.0\.0\.1)(:\d+)?(\/|$)/i.test(t)) return `http://${t}`;
  if (!/\s/.test(t) && /^[\w-]+(\.[\w-]+)+(:\d+)?(\/\S*)?$/.test(t)) return `https://${t}`;
  return `https://duckduckgo.com/?q=${encodeURIComponent(t)}`;
}

export function isExplicitUrl(raw: string): boolean {
  const t = raw.trim();
  return /^https?:\/\/\S+$/i.test(t) || /^(localhost|127\.0\.0\.1):\d+(\/\S*)?$/i.test(t);
}

export function folderName(path: string): string {
  const parts = path.replace(/[/\\]+$/, "").split(/[/\\]/);
  return parts[parts.length - 1] || path;
}

export function isSystemTab(t: Tab): boolean {
  return t.kind !== "session" && t.kind !== "page";
}
