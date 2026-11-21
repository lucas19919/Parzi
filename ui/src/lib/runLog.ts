import type { UiEvent } from "./api";

/** One tool step of a run, as the Running card lists it. */
export interface LogStep {
  id: string;
  label: string;
  at: number;
  running: boolean;
  ok: boolean;
  ms: number;
}

/** A session's latest run, step by step. It stays after the run ends, until
 *  the next run's first step replaces it. */
export interface RunLog {
  steps: LogStep[];
  ended: boolean;
}

/** The most steps a log keeps; older ones drop off the top. */
export const LOG_MAX = 50;

export function stepLog(log: RunLog | undefined, e: UiEvent, now = Date.now()): RunLog | undefined {
  if (e.kind === "tool_call") {
    const fresh = !log || log.ended;
    const steps = fresh ? [] : log.steps;
    if (steps.some((s) => s.id === e.id)) return log;
    const step = { id: e.id, label: e.label || e.name, at: now, running: true, ok: true, ms: 0 };
    return { steps: [...steps, step].slice(-LOG_MAX), ended: false };
  }
  if (!log) return log;
  if (e.kind === "tool_result") {
    if (!log.steps.some((s) => s.id === e.id && s.running)) return log;
    return { ...log, steps: log.steps.map((s) => (s.id === e.id && s.running ? { ...s, running: false, ok: e.ok, ms: e.ms } : s)) };
  }
  if (e.kind === "done" || e.kind === "error" || e.kind === "paused") {
    // A step still open when the run ends: finished with it if the run
    // completed, cut off if it failed or stopped.
    const ok = e.kind === "done";
    return { ended: true, steps: log.steps.map((s) => (s.running ? { ...s, running: false, ok } : s)) };
  }
  return log;
}
