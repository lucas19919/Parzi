import { getCurrentWindow } from "@tauri-apps/api/window";
import { distance, windowDown, windowMove } from "./gestures";

export async function windowMinimize() {
  await getCurrentWindow().minimize();
}

export async function windowMaximize() {
  await getCurrentWindow().toggleMaximize();
}

export async function windowClose() {
  await getCurrentWindow().close();
}

// One armed gesture. A second press replaces it: the old listeners must not
// call startDragging after the button is already up.
let startX = 0;
let startY = 0;

function disarm() {
  window.removeEventListener("mousemove", onMouseMove);
  window.removeEventListener("mouseup", onPointerDone);
  window.removeEventListener("pointerup", onPointerDone);
  window.removeEventListener("pointercancel", onPointerDone);
  window.removeEventListener("blur", disarm);
}

function onMouseMove(moveEvent: MouseEvent) {
  const action = windowMove(moveEvent.buttons, distance(startX, startY, moveEvent.clientX, moveEvent.clientY));
  if (action === "wait") return;
  // Drop before the IPC call. startDragging after the button is up leaves
  // Windows in a modal drag loop that eats every click until the next one.
  disarm();
  if (action === "drag") {
    try {
      void getCurrentWindow().startDragging();
    } catch {}
  }
}

function onPointerDone() {
  disarm();
}

export function startWindowDrag(e: MouseEvent) {
  const action = windowDown(e.button, e.detail, e.target);
  if (action === "ignore") return;
  disarm();
  if (action === "maximize") {
    void windowMaximize();
    return;
  }
  startX = e.clientX;
  startY = e.clientY;
  window.addEventListener("mousemove", onMouseMove);
  window.addEventListener("mouseup", onPointerDone);
  window.addEventListener("pointerup", onPointerDone);
  window.addEventListener("pointercancel", onPointerDone);
  window.addEventListener("blur", disarm);
}
