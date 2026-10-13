// How long ago, in a few words: "just now", "5m ago", "yesterday", "Oct 3".
export function ago(at: number, now = Date.now()): string {
  if (!Number.isFinite(at) || at <= 0) return "";
  const s = (now - at) / 1000;
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  if (s < 172800) return "yesterday";
  return new Date(at).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
