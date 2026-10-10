export type ComposerMode = "search" | "build" | "work";
type TabKind = "session" | "page" | "brain" | "history" | "settings" | "preview" | "remote";

export interface TabComposerState {
  input: string;
  mode: ComposerMode;
  model: string;
  effort: string;
  attachments: string[];
}

function defaultComposer(): TabComposerState {
  return { input: "", mode: "build", model: "", effort: "medium", attachments: [] };
}

export interface Tab {
  id: string;
  kind: TabKind;
  title: string;
  sessionId?: string | null;
  url?: string;
  cwd?: string;
  owner?: string | null;
  loading?: boolean;
  canGoBack?: boolean;
  canGoForward?: boolean;
  blocked?: number;
  bg?: string;
  composer: TabComposerState;
}

let seq = 0;

function tabId(): string {
  seq += 1;
  return `tab-${Date.now().toString(36)}-${seq}`;
}

export function sessionTab(sessionId: string | null = null, title = "New session"): Tab {
  return { id: tabId(), kind: "session", title, sessionId, composer: defaultComposer() };
}

export function pageTab(url = "", id = tabId(), owner: string | null = null): Tab {
  return { id, kind: "page", title: hostOf(url) || "New page", url, owner, composer: defaultComposer() };
}

export function historyTab(): Tab {
  return { id: tabId(), kind: "history", title: "History", composer: defaultComposer() };
}

export function brainTab(): Tab {
  return { id: tabId(), kind: "brain", title: "Brain", composer: defaultComposer() };
}

export function remoteTab(): Tab {
  return { id: tabId(), kind: "remote", title: "Remote", composer: defaultComposer() };
}

export function settingsTab(): Tab {
  return { id: tabId(), kind: "settings", title: "Settings", composer: defaultComposer() };
}

export function previewTab(which = "omnibar"): Tab {
  return { id: tabId(), kind: "preview", title: `Preview ${which}`, url: which, composer: defaultComposer() };
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

/// Brain/history/settings/preview live apart from working tabs.
export function isSystemTab(t: Tab): boolean {
  return t.kind !== "session" && t.kind !== "page";
}
