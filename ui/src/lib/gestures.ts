
export const DRAG_START = 4;
export const PRESS_SLOP = 5;

export const WINDOW_DRAG_SKIP =
  "button, input, textarea, select, a, label, [role='tab'], [role='menuitem'], [role='menu'], [role='dialog'], [contenteditable], .tab, .tabs, .dropdown, .picker, .home-menu, .menu-pop";

type WithClosest = { closest: (selector: string) => unknown };

function asElement(target: EventTarget | null): WithClosest | null {
  const el = target as unknown as WithClosest | null;
  if (el && typeof el.closest === "function") return el;
  return null;
}

export type WindowDown = "ignore" | "maximize" | "arm";

export function windowDown(button: number, detail: number, target: EventTarget | null): WindowDown {
  if (button !== 0) return "ignore";
  const el = asElement(target);
  if (!el || el.closest(WINDOW_DRAG_SKIP)) return "ignore";
  if (detail >= 2) return "maximize";
  return "arm";
}

export type WindowMove = "wait" | "drop" | "drag";

export function windowMove(buttons: number, dist: number): WindowMove {
  if ((buttons & 1) === 0) return "drop";
  if (dist < DRAG_START) return "wait";
  return "drag";
}

export function distance(x0: number, y0: number, x1: number, y1: number): number {
  return Math.hypot(x1 - x0, y1 - y0);
}

export function dragPast(distancePx: number, start = DRAG_START): boolean {
  return distancePx >= start;
}

export function commitWindowDrag(buttonDown: boolean): boolean {
  return buttonDown;
}

export function buttonHeld(buttons: number): boolean {
  return (buttons & 1) !== 0;
}

export function tabDragStarted(x0: number, y0: number, x1: number, y1: number, vertical: boolean): boolean {
  const delta = vertical ? y1 - y0 : x1 - x0;
  return Math.abs(delta) >= PRESS_SLOP;
}

export interface TabGesture {
  pressId: string;
  dragId: string;
  order: string[] | null;
  ids: string[];
}

export type TabRelease =
  | { kind: "pending" }
  | { kind: "select"; id: string }
  | { kind: "move"; id: string; to: number }
  | { kind: "none" };

export function tabRelease(g: TabGesture): TabRelease {
  if (g.dragId && g.order) {
    const to = g.order.indexOf(g.dragId);
    const from = g.ids.indexOf(g.dragId);
    if (to >= 0 && from >= 0 && to !== from) return { kind: "move", id: g.dragId, to };
    return { kind: "select", id: g.dragId };
  }
  if (g.pressId) return { kind: "pending" };
  return { kind: "none" };
}

export function noteMoveStarts(movedPx: number, overPath: string | null, pressPath: string): boolean {
  return movedPx >= PRESS_SLOP && overPath !== pressPath;
}

export interface NotePress {
  path: string;
  from: string;
}

export type NoteRelease =
  | { kind: "open"; path: string }
  | { kind: "move"; path: string; from: string; to: string; copy: boolean }
  | { kind: "cancel" };

export function noteRelease(press: NotePress, dragging: boolean, dropOn: string, upPath: string | null, copy: boolean): NoteRelease {
  if (dragging && dropOn && dropOn !== press.from) return { kind: "move", path: press.path, from: press.from, to: dropOn, copy };
  if (upPath === press.path) return { kind: "open", path: press.path };
  return { kind: "cancel" };
}

export interface PressDrag<P> {
  press(e: PointerEvent): P | null;
  starts(p: P, e: PointerEvent): boolean;
  start?(p: P, e: PointerEvent): void;
  drag?(p: P, e: PointerEvent): void;
  release(p: P, e: PointerEvent, dragged: boolean): void;
  cancel?(p: P): void;
}

export function pressDrag<P>(node: HTMLElement, opts: PressDrag<P>) {
  let o = opts;
  let live: { p: P; pointer: number; dragging: boolean } | null = null;

  function listen(on: boolean) {
    const set = on ? window.addEventListener.bind(window) : window.removeEventListener.bind(window);
    set("pointerdown", onDownAnywhere, true);
    set("pointermove", onMove);
    set("pointerup", onUp);
    set("pointercancel", cancel);
    set("lostpointercapture", onLost);
    set("blur", cancel);
  }

  function end() {
    const was = live;
    live = null;
    listen(false);
    return was;
  }

  function cancel() {
    const was = end();
    if (was) o.cancel?.(was.p);
  }

  function release(e: PointerEvent) {
    const was = end();
    if (was) o.release(was.p, e, was.dragging);
  }

  function onDown(e: PointerEvent) {
    if (e.button !== 0) return;
    const p = o.press(e);
    if (p === null) return;
    live = { p, pointer: e.pointerId, dragging: false };
    listen(true);
  }

  function onDownAnywhere() {
    if (live) cancel();
  }

  function onMove(e: PointerEvent) {
    if (!live || e.pointerId !== live.pointer) return;
    if (!buttonHeld(e.buttons)) {
      release(e);
      return;
    }
    if (!live.dragging) {
      if (!o.starts(live.p, e)) return;
      live.dragging = true;
      o.start?.(live.p, e);
    }
    o.drag?.(live.p, e);
  }

  function onUp(e: PointerEvent) {
    if (!live || e.pointerId !== live.pointer) return;
    release(e);
  }

  function onLost(e: PointerEvent) {
    if (!live || e.pointerId !== live.pointer || !buttonHeld(e.buttons)) return;
    cancel();
  }

  node.addEventListener("pointerdown", onDown);
  return {
    update(next: PressDrag<P>) {
      o = next;
    },
    destroy() {
      node.removeEventListener("pointerdown", onDown);
      cancel();
    },
  };
}
