import { getCurrentWindow } from "@tauri-apps/api/window";

export async function windowMinimize() {
  await getCurrentWindow().minimize();
}

export async function windowMaximize() {
  await getCurrentWindow().toggleMaximize();
}

export async function windowClose() {
  await getCurrentWindow().close();
}

// Dragging must start synchronously inside mousedown: any IPC roundtrip
// first (including our own backend command) misses the gesture and the
// window never moves. No data-tauri-drag-region either: it fights clicks.
export function startWindowDrag(e: MouseEvent) {
  if (e.button !== 0) return;
  if ((e.target as HTMLElement).closest("button, input, a, [role='tab'], .tab")) return;
  try {
    void getCurrentWindow().startDragging();
  } catch {}
}
