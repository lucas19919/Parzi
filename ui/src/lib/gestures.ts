// Clicks and drags share one rule: a click stays a click. Starting a drag on
// the press, or leaving the gesture armed after the button is up, holds
// pointer capture (and, for the window, Windows' modal drag loop) so the
// next click never lands.

// Window and card drags. A few pixels of jitter must not count.
export const DRAG_START = 4;
// Tabs and notes sit in dense lists; they get one more pixel of slop.
export const PRESS_SLOP = 5;

// Chrome that must not move the window. Popovers are drawn inside the top
// bar, so a press on their padding is still a press on the popover. The tab
// strip is too: a press on the gap between pills is not a window drag.
export const WINDOW_DRAG_SKIP =
  "button, input, textarea, select, a, label, [role='tab'], [role='menuitem'], [role='menu'], [role='dialog'], [contenteditable], .tab, .tabs, .dropdown, .noti-popover, .picker, .home-menu, .menu-pop";

type WithClosest = { closest: (selector: string) => unknown };

function asElement(target: EventTarget | null): WithClosest | null {
  const el = target as unknown as WithClosest | null;
  if (el && typeof el.closest === "function") return el;
  // A text node has no closest(). Dragging from one freezes the gesture, so refuse.
  return null;
}

export type WindowDown = "ignore" | "maximize" | "arm";

export function windowDown(button: number, detail: number, target: EventTarget | null): WindowDown {
  if (button !== 0) return "ignore";
  const el = asElement(target);
  if (!el || el.closest(WINDOW_DRAG_SKIP)) return "ignore";
  // The second click of a double-click. The first already armed and disarmed.
  if (detail >= 2) return "maximize";
  return "arm";
}

export type WindowMove = "wait" | "drop" | "drag";

// `buttons` is the MouseEvent bitfield. Bit 0 is the primary button.
export function windowMove(buttons: number, dist: number): WindowMove {
  if ((buttons & 1) === 0) return "drop";
  if (dist < DRAG_START) return "wait";
  return "drag";
}

export function distance(x0: number, y0: number, x1: number, y1: number): number {
  return Math.hypot(x1 - x0, y1 - y0);
}

// How far a card header moves before it is a drag. This never takes pointer
// capture: on Windows a capture the page window steals is not returned, and
// the next click never lands. Drags listen on the window instead.
export function dragPast(distancePx: number, start = DRAG_START): boolean {
  return distancePx >= start;
}

// The page calls the native drag after a few pixels. By then the button may
// already be up. Starting the drag in that state makes Windows wait for a
// later click, and that click never reaches the shell.
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

// `pending`: the click handler selects. `select`: the drag captured the
// pointer, so the click on the tab never fires and the release selects.
// `move`: the order changed.
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

// A note's row lives inside its folder, so a few pixels of movement still
// hit that folder. That is a click, not a drag, until the pointer leaves the row.
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
