import { writable } from "svelte/store";

interface Toast {
  id: number;
  text: string;
  err: boolean;
  session?: string;
  // A toast that updates in place (a download) carries a key.
  key?: string;
  // 0..1 shows a progress bar.
  progress?: number;
  action?: { label: string; run: () => void };
}

export const toasts = writable<Toast[]>([]);

let seq = 0;
const timers = new Map<number, number>();

function expire(id: number, ms: number) {
  clearTimeout(timers.get(id));
  timers.delete(id);
  if (ms > 0) timers.set(id, window.setTimeout(() => dismissToast(id), ms));
}

function push(text: string, err: boolean, session: string | undefined, ms: number) {
  const id = ++seq;
  toasts.update((all) => [...all, { id, text, err, session }]);
  expire(id, ms);
}

export function dismissToast(id: number) {
  clearTimeout(timers.get(id));
  timers.delete(id);
  toasts.update((all) => all.filter((t) => t.id !== id));
}

/**
 * Shows or updates the toast with this key. `ms` 0 keeps it until the next
 * update; `null` text removes it.
 */
export function keyedToast(
  key: string,
  text: string | null,
  opts: { err?: boolean; progress?: number; action?: Toast["action"]; ms?: number } = {},
) {
  let id = 0;
  toasts.update((all) => {
    const prev = all.find((t) => t.key === key);
    if (text === null) {
      if (prev) id = prev.id;
      return all.filter((t) => t.key !== key);
    }
    const next: Toast = { id: prev?.id ?? ++seq, key, text, err: !!opts.err, progress: opts.progress, action: opts.action };
    id = next.id;
    return prev ? all.map((t) => (t.key === key ? next : t)) : [...all, next];
  });
  if (text === null) {
    if (id) expire(id, 0);
    return;
  }
  expire(id, opts.ms ?? (opts.err ? 5000 : 3000));
}

export function toast(text: string, err = false) {
  push(text, err, undefined, err ? 5000 : 3000);
}

export function notify(text: string, session: string) {
  push(text, false, session, 6000);
}

export function toastError(e: unknown) {
  toast(String(e), true);
}

let ctx: AudioContext | null = null;

// Short two-tone bell for background completions. Best effort: blocked
// audio stays silent instead of throwing.
export function chime(ok: boolean) {
  try {
    ctx ??= new AudioContext();
    if (ctx.state === "suspended") {
      void ctx.resume().catch(() => {});
      return;
    }
    const t = ctx.currentTime;
    for (const [i, f] of (ok ? [660, 880] : [440, 330]).entries()) {
      const o = ctx.createOscillator();
      const g = ctx.createGain();
      o.type = "sine";
      o.frequency.value = f;
      g.gain.setValueAtTime(0.0001, t + i * 0.14);
      g.gain.exponentialRampToValueAtTime(0.18, t + i * 0.14 + 0.02);
      g.gain.exponentialRampToValueAtTime(0.0001, t + i * 0.14 + 0.3);
      o.connect(g).connect(ctx.destination);
      o.start(t + i * 0.14);
      o.stop(t + i * 0.14 + 0.32);
    }
  } catch {}
}
