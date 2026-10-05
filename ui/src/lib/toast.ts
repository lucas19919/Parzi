import { writable } from "svelte/store";

interface Toast {
  id: number;
  text: string;
  err: boolean;
}

export const toasts = writable<Toast[]>([]);

let seq = 0;

export function toast(text: string, err = false) {
  const id = ++seq;
  toasts.update((all) => [...all, { id, text, err }]);
  setTimeout(() => toasts.update((all) => all.filter((t) => t.id !== id)), err ? 5000 : 3000);
}

export function toastError(e: unknown) {
  toast(String(e), true);
}
