/** Window controls and dragging for frameless Tauri window. */
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
