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
