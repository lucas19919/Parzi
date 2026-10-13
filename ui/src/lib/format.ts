// Short text for live and finished work: file names for paths, clipped
// lines, elapsed times and costs.
import type { RunEnd } from "./api";

export function basename(path: string): string {
  const parts = path.replace(/[\\/]+$/, "").split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

// A tool label as one short line: paths read as their file name
// ("Editing D:\\src\\Omnibar.svelte" is "Editing Omnibar.svelte").
export function shortLabel(label: string): string {
  return label
    .replace(/`/g, "")
    .replace(/\s+/g, " ")
    .trim()
    .split(" ")
    .map((w) => (/^[a-z]+:\/\//i.test(w) || !/[\\/]/.test(w) || w.length < 3 ? w : basename(w)))
    .join(" ");
}

// At most `max` characters, cut at a word where it can be.
export function clip(text: string, max: number): string {
  if (text.length <= max) return text;
  const cut = text.slice(0, max - 1);
  const space = cut.lastIndexOf(" ");
  return `${(space > max * 0.6 ? cut.slice(0, space) : cut).trimEnd()}…`;
}

// The first line of a run's summary, without markdown marks.
export function firstLine(text: string | null | undefined): string {
  for (const raw of (text ?? "").split("\n")) {
    const line = plainLine(raw);
    if (line) return line;
  }
  return "";
}

// A summary as plain prose: markdown marks and code fences dropped, lines
// joined.
export function plainText(text: string | null | undefined): string {
  return (text ?? "")
    .replace(/```[\s\S]*?(```|$)/g, " ")
    .split("\n")
    .map(plainLine)
    .filter(Boolean)
    .join(" ");
}

function plainLine(raw: string): string {
  return raw
    .replace(/^\s*(#+|[-*>]|\d+\.)\s+/, "")
    .replace(/[*`]/g, "")
    // Emphasis underscores, not the ones inside names (name_from_prompt).
    .replace(/(?<!\w)_|_(?!\w)/g, "")
    .trim();
}

export function formatElapsed(ms: number): string {
  const s = Math.max(0, Math.round(ms / 1000));
  if (s < 60) return `${s}s`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m`;
  const h = Math.floor(m / 60);
  if (h < 24) return m % 60 ? `${h}h ${m % 60}m` : `${h}h`;
  const d = Math.floor(h / 24);
  return h % 24 ? `${d}d ${h % 24}h` : `${d}d`;
}

export function formatCost(usd: number | null | undefined): string {
  if (usd === null || usd === undefined || !Number.isFinite(usd) || usd <= 0) return "";
  if (usd < 0.01) return "<$0.01";
  if (usd < 100) return `$${usd.toFixed(2)}`;
  return `$${Math.round(usd)}`;
}

export function ago(at: number, now = Date.now()): string {
  const s = (now - at) / 1000;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

const RUN_WORDS: Record<RunEnd, string> = {
  completed: "Finished",
  failed: "Failed",
  paused: "Paused: budget",
  stopped: "Stopped",
  crashed: "Crashed",
  interrupted: "Cut off",
  not_started: "Didn't start",
};

export function runWord(end: RunEnd): string {
  return RUN_WORDS[end] ?? end;
}
