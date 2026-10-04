import { getCurrentWindow } from "@tauri-apps/api/window";

import { api } from "./api";

export async function windowMinimize() {
  await getCurrentWindow().minimize();
}

export async function windowMaximize() {
  await getCurrentWindow().toggleMaximize();
}

export async function windowClose() {
  await getCurrentWindow().close();
}

export function startWindowDrag(e: MouseEvent) {
  if (e.button !== 0 || (e.target as HTMLElement).closest("button, input, a, [role='tab']")) return;
  api.windowStartDragging().catch(() => {
    try {
      void getCurrentWindow().startDragging();
    } catch {}
  });
}
