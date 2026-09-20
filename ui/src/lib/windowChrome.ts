/** Window controls and dragging for frameless Tauri window. Controls go
    straight to the webview API; dragging goes through the shell command
    (it carries the Tauri window) with the same fallback. */
import { getCurrentWindow } from "@tauri-apps/api/window";

import { api } from "./api";

export const WIN_ICON = {
  min: "M5 12h14",
  max: "M5 5h14v14H5z",
  close: "M18 6L6 18M6 6l12 12",
};

export async function windowMinimize() {
  await getCurrentWindow().minimize();
}

export async function windowMaximize() {
  await getCurrentWindow().toggleMaximize();
}

export async function windowClose() {
  await getCurrentWindow().close();
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
