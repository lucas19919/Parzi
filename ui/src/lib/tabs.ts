export type TabKind = "session" | "page" | "brain" | "history";

export interface Tab {
  id: string;
  kind: TabKind;
  title: string;
  sessionId?: string | null;
  url?: string;
  cwd?: string;
  loading?: boolean;
  canGoBack?: boolean;
  canGoForward?: boolean;
  blocked?: number;
  bg?: string;
}

let seq = 0;

export function tabId(): string {
  seq += 1;
  return `tab-${Date.now().toString(36)}-${seq}`;
}

export function sessionTab(sessionId: string | null = null, title = "New session"): Tab {
  return { id: tabId(), kind: "session", title, sessionId };
}

export function pageTab(url = "", id = tabId()): Tab {
  return { id, kind: "page", title: hostOf(url) || "New page", url };
}

export function historyTab(): Tab {
  return { id: tabId(), kind: "history", title: "History" };
}

export function brainTab(): Tab {
  return { id: tabId(), kind: "brain", title: "Brain" };
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
