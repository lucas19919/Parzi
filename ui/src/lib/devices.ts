/** The phones a page can be shown as, and how big that phone is drawn. */

export type DeviceId = "laptop" | "iphone" | "android";

export interface Phone {
  label: string;
  /** The screen in CSS pixels, portrait. */
  width: number;
  height: number;
  /** Device pixels per CSS pixel. */
  scale: number;
  userAgent: string;
}

export const PHONES: Record<Exclude<DeviceId, "laptop">, Phone> = {
  iphone: {
    label: "iPhone",
    width: 393,
    height: 852,
    scale: 3,
    userAgent:
      "Mozilla/5.0 (iPhone; CPU iPhone OS 18_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.1 Mobile/15E148 Safari/604.1",
  },
  android: {
    label: "Android",
    width: 412,
    height: 915,
    scale: 2.625,
    userAgent:
      "Mozilla/5.0 (Linux; Android 15; Pixel 9) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Mobile Safari/537.36",
  },
};

/** The frame's edge around the screen, in CSS pixels at full size. */
export const BEZEL = 12;

/** How far a phone is scaled down to fit `room` (the space the page has),
 *  frame included: 1 when it fits at full size, never above 1, rounded to
 *  hundredths so small resizes don't make the page relayout. */
export function phoneFit(phone: Phone, room: { width: number; height: number }): number {
  const w = (room.width - 2 * BEZEL) / phone.width;
  const h = (room.height - 2 * BEZEL) / phone.height;
  const fit = Math.min(1, w, h);
  return fit > 0 ? Math.floor(fit * 100) / 100 : 0.1;
}
