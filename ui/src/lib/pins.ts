import type { HostJob, ProjectPlan } from "./api";

/** A link the session keeps coming back to, shown above the composer. */
export interface Pin {
  kind: "url" | "file";
  label: string;
  target: string;
}

const LOCAL = new Set(["localhost", "127.0.0.1", "[::1]"]);

/** The address of a URL on this computer, as "host:port", or "" for any other. */
function localHost(url: string): string {
  try {
    const u = new URL(url);
    return /^https?:$/.test(u.protocol) && LOCAL.has(u.hostname) ? u.host : "";
  } catch {
    return "";
  }
}

/** The session's pins: each running dev server's first localhost address,
 *  then the plan's document when it has one. */
export function sessionPins(hosts: HostJob[], planDoc?: string | null): Pin[] {
  const out: Pin[] = [];
  const seen = new Set<string>();
  for (const h of hosts) {
    if (!h.running) continue;
    const url = h.urls.find((u) => localHost(u));
    if (!url || seen.has(url)) continue;
    seen.add(url);
    out.push({ kind: "url", label: localHost(url), target: url });
  }
  if (planDoc?.trim()) out.push({ kind: "file", label: "Plan", target: planDoc.trim() });
  return out;
}

export interface TargetsSummary {
  open: { title: string; working: boolean }[];
  done: number;
}

/** What the targets row shows: the targets still to do, in order, and how
 *  many are done. Null when there is no plan or it has no targets. */
export function targetsSummary(plan: ProjectPlan | null | undefined): TargetsSummary | null {
  const targets = Array.isArray(plan?.targets) ? plan.targets : [];
  if (!targets.length) return null;
  const open = targets.filter((t) => t.state !== "done").map((t) => ({ title: t.title.trim() || "Untitled", working: t.state === "working" }));
  return { open, done: targets.length - open.length };
}
