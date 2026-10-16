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

// Dragging starts once the pointer has moved slightly while holding the button:
// calling startDragging on pure mousedown races with mouseup, leaving Windows in
// a modal drag loop that freezes UI input until another click.
export function startWindowDrag(e: MouseEvent) {
  if (e.button !== 0) return;
  if ((e.target as HTMLElement).closest("button, input, a, [role='tab'], .tab")) return;
  if (e.detail === 2) {
    void windowMaximize();
    return;
  }
  const startX = e.clientX;
  const startY = e.clientY;
  let dragging = false;

  const onMouseMove = (moveEvent: MouseEvent) => {
    if (dragging) return;
    if (moveEvent.buttons !== 1) {
      cleanup();
      return;
    }
    const dist = Math.hypot(moveEvent.clientX - startX, moveEvent.clientY - startY);
    if (dist >= 4) {
      dragging = true;
      cleanup();
      try {
        void getCurrentWindow().startDragging();
      } catch {}
    }
  };

  const onMouseUp = () => {
    cleanup();
  };

  const cleanup = () => {
    window.removeEventListener("mousemove", onMouseMove);
    window.removeEventListener("mouseup", onMouseUp);
  };

  window.addEventListener("mousemove", onMouseMove);
  window.addEventListener("mouseup", onMouseUp);
}
