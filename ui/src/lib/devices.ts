/** The phones a page can be shown as, and how big that phone is drawn. */

export type DeviceId = "laptop" | "iphone" | "android" | "duo";
export type PhoneId = Exclude<DeviceId, "laptop">;

/** How a phone is drawn: its body, camera, hinge and buttons. */
export type Art = "iphone" | "android" | "duo-open" | "duo-open-portrait" | "duo-closed";

/** The iPhone Duo's physical states: unfolded wide, unfolded rotated, or folded. */
export type DuoPose = "open" | "portrait" | "closed";

export interface Phone {
  label: string;
  /** The toolbar tooltip. */
  tip: string;
  art: Art;
  /** The whole screen in CSS pixels (logical points). */
  width: number;
  height: number;
  /** Device pixels per CSS pixel. */
  scale: number;
  userAgent: string;
  /** Insets reserved inside the screen for system chrome (status bar, home
   *  indicator, or the closed Duo's 84pt right bar), painted in the page's
   *  colour: the page sits inside them so the camera, rail and round screen
   *  corners never overlap the page. */
  top: number;
  bottom: number;
  left?: number;
  right?: number;
  /** The screen's corner radius (outer/right corners when asymmetric). */
  radius: number;
  /** Hinge-side (left) corner radius when asymmetric (the closed Duo's 8pt edge). */
  leftRadius?: number;
  /** Black glass between the screen and the metal edge, and the edge itself. */
  bezel: number;
  edge: number;
  /** Extra titanium hinge spine width along the left edge (closed Duo). */
  spine?: number;
}

// iOS 27 / Safari 27.1 sends the frozen iPhone OS 18_7 token on both slab iPhones
// and the iPhone Duo (no foldable-specific token).
const IPHONE_UA =
  "Mozilla/5.0 (iPhone; CPU iPhone OS 18_7 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/27.0 Mobile/15E148 Safari/604.1";

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
  // The iPhone Duo unfolded (native landscape): 7.6-inch inner display.
  // iOS 27 renders at 951x669 pt (2853x2007 px at 3x, scaled to 2670x1878)
  // with 55pt symmetric corners and an under-display camera. Full-screen
  // Safari spans all 951 CSS px between the top bar and home indicator.
  duo: {
    label: "Duo",
    tip: "As on an iPhone Duo",
    art: "duo-open",
    width: 951,
    height: 669,
    scale: 3,
    userAgent: IPHONE_UA,
    top: 50,
    bottom: 46,
    radius: 55,
    bezel: 14,
    edge: 6,
  },
};

/** The iPhone Duo unfolded and rotated to portrait: 669x951 pt at 3x, with
 *  the hinge running horizontally and a traditional top status bar. */
export const DUO_PORTRAIT: Phone = {
  ...PHONES.duo,
  art: "duo-open-portrait",
  width: 669,
  height: 951,
  top: 48,
  bottom: 46,
};

/** The iPhone Duo folded: its 5.4-inch passport outer screen (466x678 pt,
 *  1398x2034 px at 3x) with asymmetric corners (8pt at the left titanium hinge
 *  spine, 59pt on the outer right edge). Safari 27.1 reserves an 84pt vertical
 *  bar on the right for the vertical Dynamic Island and circular status module,
 *  leaving 382x618 CSS px for pages. */
export const DUO_CLOSED: Phone = {
  ...PHONES.duo,
  art: "duo-closed",
  width: 466,
  height: 678,
  top: 24,
  bottom: 36,
  right: 84,
  radius: 59,
  leftRadius: 8,
  bezel: 10,
  edge: 5,
  spine: 8,
};

export function phoneFor(id: PhoneId, pose: DuoPose | boolean = "open"): Phone {
  if (id !== "duo") return PHONES[id];
  if (pose === true || pose === "closed") return DUO_CLOSED;
  if (pose === "portrait") return DUO_PORTRAIT;
  return PHONES.duo;
}

/** How far a corner of the page rectangle reaches outside its rounded screen
 *  corner at full size: under a pixel for every phone and pose. */
export function cornerOverhang(p: Phone): number {
  const left = p.left ?? 0;
  const right = p.right ?? 0;
  const rL = p.leftRadius ?? p.radius;
  const rR = p.radius;
  const corners: Array<[number, number, number]> = [
    [rL, left, p.top],
    [rL, left, p.bottom],
    [rR, right, p.top],
    [rR, right, p.bottom],
  ];
  let max = 0;
  for (const [r, dx, dy] of corners) {
    if (dx >= r || dy >= r) continue;
    const rise = r - dy;
    const reach = r - dx - Math.sqrt(r * r - rise * rise);
    if (reach > max) max = reach;
  }
  return max;
}

/** What the page gets: the screen inside its system bars and side rail. */
export function viewport(p: Phone): { width: number; height: number } {
  return {
    width: p.width - (p.left ?? 0) - (p.right ?? 0),
    height: p.height - p.top - p.bottom,
  };
}

/** How far the side buttons stand out of the body, at full size. */
export const BUTTON = 3;

/** Standard distance from the drawing edge to the screen, at full size. */
export function screenAt(p: Phone): number {
  return BUTTON + p.edge + p.bezel;
}

/** Where the screen starts inside the drawing, accounting for a left hinge spine. */
export function screenOrigin(p: Phone): { x: number; y: number } {
  const at = screenAt(p);
  return { x: at + (p.spine ?? 0), y: at };
}

/** The whole drawing (buttons, hinge spine, body, screen) at full size. */
export function frameSize(p: Phone): { width: number; height: number } {
  const around = 2 * screenAt(p);
  return { width: p.width + around + (p.spine ?? 0), height: p.height + around };
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
