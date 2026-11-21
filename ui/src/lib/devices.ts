/** The phones a page can be shown as, and how big that phone is drawn. */

export type DeviceId = "laptop" | "iphone" | "android" | "fold";
export type PhoneId = Exclude<DeviceId, "laptop">;

/** How a phone is drawn: its body, camera and buttons. */
export type Art = "iphone" | "android" | "fold-open" | "fold-closed";

export interface Phone {
  label: string;
  /** The toolbar tooltip. */
  tip: string;
  art: Art;
  /** The whole screen in CSS pixels, portrait. */
  width: number;
  height: number;
  /** Device pixels per CSS pixel. */
  scale: number;
  userAgent: string;
  /** The status bar and the home indicator area, drawn inside the screen in
   *  the page's colour: the page sits between them, as in Safari or Chrome,
   *  so the camera and the round screen corners are never over the page. */
  top: number;
  bottom: number;
  /** The screen's corner radius. The page is a sharp rectangle, so the
   *  bars are deep enough that its corners stay inside the round corners. */
  radius: number;
  /** Black glass between the screen and the metal edge, and the edge itself. */
  bezel: number;
  edge: number;
}

const IPHONE_UA =
  "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1";

export const PHONES: Record<PhoneId, Phone> = {
  // iPhone 16 Pro: 402x874 points at 3x; the status bar is its 62pt top safe
  // area. Its home indicator area is 34pt, here 40 so the page's corners
  // clear the screen's. 15pt of glass and titanium per side makes the
  // 71.5mm body.
  iphone: {
    label: "iPhone",
    tip: "As on an iPhone 16 Pro",
    art: "iphone",
    width: 402,
    height: 874,
    scale: 3,
    userAgent: IPHONE_UA,
    top: 62,
    bottom: 40,
    radius: 49,
    bezel: 10,
    edge: 5,
  },
  // Pixel 9: 412x923 dp at 2.625 (1080x2424 pixels), 72mm wide.
  android: {
    label: "Android",
    tip: "As on a Pixel 9",
    art: "android",
    width: 412,
    height: 923,
    scale: 2.625,
    userAgent:
      "Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36",
    top: 40,
    bottom: 28,
    radius: 36,
    bezel: 16,
    edge: 6,
  },
  // Apple hasn't announced a foldable. These follow the rumoured book-style
  // iPhone Fold and are approximate: the inner screen open, FOLD_CLOSED below
  // the outer one. Its browser is Safari, so the user agent is the iPhone's.
  fold: {
    label: "Fold",
    tip: "As on an iPhone Fold (approx.: Apple hasn't announced one)",
    art: "fold-open",
    width: 720,
    height: 900,
    scale: 3,
    userAgent: IPHONE_UA,
    top: 50,
    bottom: 30,
    radius: 30,
    bezel: 9,
    edge: 4,
  },
};

/** The rumoured iPhone Fold closed: its outer screen (approximate). */
export const FOLD_CLOSED: Phone = {
  ...PHONES.fold,
  art: "fold-closed",
  width: 360,
  height: 800,
  top: 62,
  bottom: 38,
  radius: 46,
  bezel: 10,
};

export function phoneFor(id: PhoneId, closed = false): Phone {
  return id === "fold" && closed ? FOLD_CLOSED : PHONES[id];
}

/** What the page gets: the screen between the status bar and the home indicator. */
/** How far a corner of the page reaches outside the screen's round corner,
 *  at full size: under a pixel for every phone. */
export function cornerOverhang(p: Phone): number {
  const r = p.radius;
  const rise = r - Math.min(p.top, p.bottom);
  return rise <= 0 ? 0 : r - Math.sqrt(r * r - rise * rise);
}

export function viewport(p: Phone): { width: number; height: number } {
  return { width: p.width, height: p.height - p.top - p.bottom };
}

/** How far the side buttons stand out of the body, at full size. */
export const BUTTON = 3;

/** Where the screen starts inside the drawing, at full size. */
export function screenAt(p: Phone): number {
  return BUTTON + p.edge + p.bezel;
}

/** The whole drawing (buttons, body, screen) at full size. */
export function frameSize(p: Phone): { width: number; height: number } {
  const around = 2 * screenAt(p);
  return { width: p.width + around, height: p.height + around };
}

/** Space kept free around the drawing (not scaled), for its shadow. */
export const MARGIN = 16;

/** How far a phone is scaled down to fit `room` (the space the page has),
 *  whole drawing included: 1 when it fits at full size, never above 1,
 *  rounded to hundredths so small resizes don't make the page relayout. */
export function phoneFit(phone: Phone, room: { width: number; height: number }): number {
  const size = frameSize(phone);
  const w = (room.width - 2 * MARGIN) / size.width;
  const h = (room.height - 2 * MARGIN) / size.height;
  const fit = Math.min(1, w, h);
  return fit > 0 ? Math.floor(fit * 100) / 100 : 0.1;
}

/** Whether the status bar over `bg` (a CSS colour: rgb(), rgba() or hex)
 *  is drawn in white: on dark pages, as the phones do. */
export function lightInk(bg: string | undefined): boolean {
  const rgb = parseColor(bg ?? "");
  if (!rgb) return false;
  const [r, g, b] = rgb.map((c) => {
    const s = c / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b < 0.18;
}

function parseColor(c: string): [number, number, number] | null {
  const fn = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)/i.exec(c.trim());
  if (fn) return [Number(fn[1]), Number(fn[2]), Number(fn[3])];
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/i.exec(c.trim())?.[1];
  if (!hex) return null;
  const full = hex.length === 3 ? [...hex].map((h) => h + h).join("") : hex;
  return [0, 2, 4].map((i) => parseInt(full.slice(i, i + 2), 16)) as [number, number, number];
}
