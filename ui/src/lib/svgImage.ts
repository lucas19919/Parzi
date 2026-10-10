// SVG artifacts render as <img src="data:image/svg+xml;base64,...">: an
// image is a sealed document (no scripts, no network, and its <style>
// can't restyle the app). No imports, so node tests load it.

const SVG_NS = "http://www.w3.org/2000/svg";
const XLINK_NS = "http://www.w3.org/1999/xlink";

// Inline <svg> markup may skip the namespaces a standalone image needs.
export function withNamespaces(svg: string): string {
  return svg.replace(/<svg\b[^>]*>/i, (open) => {
    let tag = open;
    if (!/\sxmlns\s*=/i.test(tag)) tag = tag.replace(/^<svg\b/i, `<svg xmlns="${SVG_NS}"`);
    if (/\bxlink:/i.test(svg) && !/\sxmlns:xlink\s*=/i.test(tag)) tag = tag.replace(/^<svg\b/i, `<svg xmlns:xlink="${XLINK_NS}"`);
    return tag;
  });
}

function base64Utf8(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let bin = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    bin += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(bin);
}

export function svgDataUrl(svg: string): string {
  const src = svg.trim();
  if (!/<svg\b/i.test(src)) return "";
  return `data:image/svg+xml;base64,${base64Utf8(withNamespaces(src))}`;
}
