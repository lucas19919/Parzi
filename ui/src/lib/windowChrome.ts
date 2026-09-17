/**
 * Minimise / maximise / close / drag for a frameless window.
 *
 * Shared because two surfaces own the top of the window at different times:
 * the titlebar normally, and the inspector deck in full view — which covers
 * the titlebar outright, so it has to carry the controls itself or the
 * window becomes undismissable from the keyboard-free path.
 *
 * Each call prefers the Rust command and falls back to the JS window API,
 * which is what the titlebar has always done.
 */
import { getCurrentWindow } from "@tauri-apps/api/window";

import { api } from "./api";

export const WIN_ICON = {
  min: "M5 12h14",
  max: "M5 5h14v14H5z",
  close: "M18 6L6 18M6 6l12 12",
};

export async function windowMinimize() {
  try {
    await api.windowMinimize();
  } catch {
    await getCurrentWindow().minimize();
  }
}

export async function windowMaximize() {
  try {
    await api.windowMaximize();
  } catch {
    await getCurrentWindow().toggleMaximize();
  }
}

export async function windowClose() {
  try {
    await api.windowClose();
  } catch {
    await getCurrentWindow().close();
  }
}

/** Drag the window from a chrome surface. Ignores clicks on buttons. */
export function startWindowDrag(e: MouseEvent) {
  if (e.button !== 0 || (e.target as HTMLElement).closest("button")) return;
  api.windowStartDragging().catch(() => {
    try {
      getCurrentWindow().startDragging();
    } catch {}
  });
}
