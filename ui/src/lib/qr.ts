/** Phone links for dev servers: which address a phone can open, and that
 *  address as a QR code to scan. */
import qrcode from "qrcode-generator";

const LOOPBACK = /^https?:\/\/(localhost|[^/]*\.localhost|127\.\d+\.\d+\.\d+|\[::1\])(:|\/|$)/i;

/** The first address a phone can reach: one on the local network or a
 *  tailnet, never localhost. Null when the server only listens locally. */
export function phoneUrl(urls: string[]): string | null {
  return urls.find((u) => /^https?:\/\//i.test(u) && !LOOPBACK.test(u)) ?? null;
}

/** The phone address of the page at `target` (a localhost URL): the
 *  network address with the same port, keeping the page's path. Null when
 *  the server prints no network address on that port. */
export function phoneFor(target: string, urls: string[]): string | null {
  let page: URL;
  try {
    page = new URL(target);
  } catch {
    return null;
  }
  const port = page.port || (page.protocol === "https:" ? "443" : "80");
  for (const u of urls) {
    if (!phoneUrl([u])) continue;
    try {
      const net = new URL(u);
      if ((net.port || (net.protocol === "https:" ? "443" : "80")) === port) {
        return `${net.origin}${page.pathname === "/" ? "" : page.pathname}${page.search}`;
      }
    } catch {
      continue;
    }
  }
  return null;
}

/** A QR code as one SVG path: `size` modules square, `d` drawing the dark
 *  ones, each row's runs merged. Draw it in a `0 0 size size` viewBox. */
export function qrPath(text: string): { size: number; d: string } {
  const qr = qrcode(0, "M");
  qr.addData(text);
  qr.make();
  const size = qr.getModuleCount();
  let d = "";
  for (let y = 0; y < size; y++) {
    let x = 0;
    while (x < size) {
      if (!qr.isDark(y, x)) {
        x++;
        continue;
      }
      const start = x;
      while (x < size && qr.isDark(y, x)) x++;
      d += `M${start} ${y}h${x - start}v1h${start - x}z`;
    }
  }
  return { size, d };
}
