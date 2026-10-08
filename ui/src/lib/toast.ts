import { writable } from "svelte/store";

interface Toast {
  id: number;
  text: string;
  err: boolean;
  session?: string;
}

export const toasts = writable<Toast[]>([]);

let seq = 0;

function push(text: string, err: boolean, session: string | undefined, ms: number) {
  const id = ++seq;
  toasts.update((all) => [...all, { id, text, err, session }]);
  setTimeout(() => toasts.update((all) => all.filter((t) => t.id !== id)), ms);
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

// Short two-tone bell for background completions. Lazy AudioContext,
// best effort: browsers block audio before first interaction, so a
// suspended context just stays silent instead of throwing.
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
