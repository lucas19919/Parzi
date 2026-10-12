// Pure rules for web pages in tabs: prompts' wording, recently closed tabs,
// favicons, downloads and where a tooltip may go while a page is shown.
// No imports, so node tests load it directly.

const PERMISSION_TEXT: Record<string, string> = {
  camera: "use your camera",
  microphone: "use your microphone",
  geolocation: "know your location",
  notifications: "show notifications",
  sensors: "use your motion sensors",
  clipboard: "see text and images you copied",
  downloads: "download several files",
  files: "edit files on your computer",
  fonts: "use the fonts on your computer",
  midi: "control your MIDI devices",
  windows: "open and place windows on your screens",
};

const PERMISSION_NAME: Record<string, string> = {
  camera: "Camera",
  microphone: "Microphone",
  geolocation: "Location",
  notifications: "Notifications",
  sensors: "Motion sensors",
  clipboard: "Clipboard",
  downloads: "Several downloads",
  files: "Files",
  fonts: "Fonts",
  midi: "MIDI devices",
  windows: "Window placement",
};

export function permissionText(kind: string): string {
  return PERMISSION_TEXT[kind] ?? "use a browser feature";
}

export function permissionName(kind: string): string {
  return PERMISSION_NAME[kind] ?? kind;
}

const APPS: Record<string, string> = {
  mailto: "your email app",
  tel: "your phone app",
  sms: "your messages app",
  zoommtg: "Zoom",
  zoomus: "Zoom",
  msteams: "Teams",
  slack: "Slack",
  vscode: "VS Code",
  "vscode-insiders": "VS Code Insiders",
  cursor: "Cursor",
  spotify: "Spotify",
  discord: "Discord",
  steam: "Steam",
  figma: "Figma",
  notion: "Notion",
  obsidian: "Obsidian",
  "ms-windows-store": "Microsoft Store",
};

/** "Zoom" for zoommtg://..., else the scheme, for "Open this link in X?". */
export function appFor(url: string): string {
  const scheme = /^([a-z][a-z0-9+.-]*):/i.exec(url)?.[1]?.toLowerCase() ?? "";
  return APPS[scheme] ?? (scheme ? `the app for ${scheme} links` : "another app");
}

export interface Closed {
  url: string;
  title: string;
  at: number;
}

/** Newest last; reopening a page drops its older entries. */
export function pushClosed(list: Closed[], entry: Closed, max = 25): Closed[] {
  if (!/^https?:\/\//i.test(entry.url)) return list;
  return [...list.filter((c) => c.url !== entry.url), entry].slice(-max);
}

export function popClosed(list: Closed[]): [Closed | null, Closed[]] {
  if (!list.length) return [null, list];
  return [list[list.length - 1], list.slice(0, -1)];
}

export function originOf(url: string): string {
  try {
    const u = new URL(url);
    return /^https?:$/.test(u.protocol) ? u.origin : "";
  } catch {
    return "";
  }
}

/** The site's real icon when a page reported one, else /favicon.ico. */
export function iconFor(icons: Record<string, string>, url: string): string {
  const origin = originOf(url);
  if (!origin) return "";
  return icons[origin] ?? `${origin}/favicon.ico`;
}

/**
 * Remembers a page's icon for its site: an https URL or a small data URL
 * (what the shell may load). Returns null when nothing changes.
 */
export function rememberIcon(icons: Record<string, string>, pageUrl: string, icon: string, max = 300): Record<string, string> | null {
  const origin = originOf(pageUrl);
  const usable = (/^https:\/\//i.test(icon) && icon.length <= 1024) || (/^data:image\/png;base64,/i.test(icon) && icon.length <= 8192);
  if (!origin || !usable || icons[origin] === icon) return null;
  const rest = Object.entries(icons).filter(([o]) => o !== origin);
  return Object.fromEntries([...rest.slice(Math.max(0, rest.length - max + 1)), [origin, icon]]);
}

export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let i = 0;
  let v = n;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v >= 10 || i === 0 ? Math.round(v) : v.toFixed(1)} ${units[i]}`;
}

export interface DownloadLike {
  state: "progress" | "done" | "failed" | "canceled";
  received: number;
  total: number;
}

/** 0..1 while the size is known, else undefined. */
export function downloadShare(d: DownloadLike): number | undefined {
  if (d.state !== "progress" || d.total <= 0) return undefined;
  return Math.max(0, Math.min(1, d.received / d.total));
}

export function downloadNote(d: DownloadLike): string {
  if (d.state === "done") return d.total > 0 ? formatBytes(d.total) : "Done";
  if (d.state === "failed") return "Failed";
  if (d.state === "canceled") return "Cancelled";
  return d.total > 0 ? `${formatBytes(d.received)} of ${formatBytes(d.total)}` : formatBytes(d.received);
}

export interface Box {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

function overlaps(a: Box, b: Box): boolean {
  return a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom;
}

/**
 * Where a tooltip goes: below its control, else above, else beside it,
 * never over `avoid` (the native page, which is drawn above all HTML).
 */
export function tipSpot(
  anchor: Box,
  tip: { width: number; height: number },
  view: { width: number; height: number },
  avoid: Box | null,
  gap = 6,
): { left: number; top: number } {
  const clampX = (x: number) => Math.max(gap, Math.min(x, view.width - tip.width - gap));
  const clampY = (y: number) => Math.max(gap, Math.min(y, view.height - tip.height - gap));
  const centerX = clampX(anchor.left + (anchor.right - anchor.left) / 2 - tip.width / 2);
  const middleY = clampY(anchor.top + (anchor.bottom - anchor.top) / 2 - tip.height / 2);
  const spots = [
    { left: centerX, top: anchor.bottom + gap, fits: anchor.bottom + gap + tip.height <= view.height - gap },
    { left: centerX, top: anchor.top - tip.height - gap, fits: anchor.top - tip.height - gap >= gap },
    { left: anchor.right + gap, top: middleY, fits: anchor.right + gap + tip.width <= view.width - gap },
    { left: anchor.left - tip.width - gap, top: middleY, fits: anchor.left - tip.width - gap >= gap },
  ];
  const clear = (s: { left: number; top: number }) =>
    !avoid || !overlaps({ left: s.left, top: s.top, right: s.left + tip.width, bottom: s.top + tip.height }, avoid);
  const best = spots.find((s) => s.fits && clear(s)) ?? spots.find((s) => s.fits) ?? spots[0];
  return { left: Math.round(best.left), top: Math.round(Math.max(gap, best.top)) };
}
