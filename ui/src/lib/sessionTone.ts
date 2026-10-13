// A session's dot on Home: running, waiting on you, failed, done, or quiet.
import type { SessionMeta } from "./api";

export type Tone = "live" | "need" | "bad" | "ok" | "idle";

export const TONE_WORDS: Record<Tone, string> = { live: "Running", need: "Needs you", bad: "Failed", ok: "Done", idle: "Idle" };

export function sessionTone(s: SessionMeta, running: ReadonlySet<string>, waiting?: { has(id: string): boolean }): Tone {
  if (waiting?.has(s.id)) return "need";
  if (running.has(s.id) || s.status === "active") return "live";
  const r = s.last_run;
  if (!r) return "idle";
  if (r.end === "failed" || r.end === "crashed" || r.end === "not_started") return "bad";
  if (r.end === "paused" || (r.checks ?? []).some((c) => !c?.ok)) return "need";
  return r.end === "completed" ? "ok" : "idle";
}
