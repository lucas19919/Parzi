export const DAY_MS = 86_400_000;

export function formatTime(at: string | number): string {
  const ms = typeof at === "string" ? (at ? Date.parse(at) : NaN) : at;
  if (!Number.isFinite(ms)) return "";
  return new Date(ms).toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
}

export function dayLabel(at: number): string {
  const start = new Date();
  start.setHours(0, 0, 0, 0);
  if (at >= start.getTime()) return "Today";
  if (at >= start.getTime() - DAY_MS) return "Yesterday";
  return new Date(at).toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" });
}

export function byDay<T extends { at: number }>(list: T[]): { label: string; items: T[] }[] {
  const out: { label: string; items: T[] }[] = [];
  for (const v of [...list].sort((a, b) => b.at - a.at)) {
    const label = dayLabel(v.at);
    const last = out[out.length - 1];
    if (last?.label === label) last.items.push(v);
    else out.push({ label, items: [v] });
  }
  return out;
}
