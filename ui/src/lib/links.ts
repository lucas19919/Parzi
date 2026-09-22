/** Clicked markdown links: never navigate the webview. https goes out to
    the browser behind one explicit confirmation per click; everything else
    is swallowed. Returns true when the click was a link (handled). */
export async function handleLinkClick(e: MouseEvent): Promise<boolean> {
  const anchor = (e.target as HTMLElement).closest?.("a") as
    | HTMLAnchorElement
    | null;
  if (!anchor) return false;
  e.preventDefault();
  e.stopPropagation();
  const href = anchor.getAttribute("href") ?? "";
  if (!/^https:\/\//i.test(href)) return true;
  const { api } = await import("./api");
  const short = href.length > 120 ? href.slice(0, 120) + "…" : href;
  if (!window.confirm(`Open in browser?\n${short}`)) return true;
  try {
    await api.openConfirmedUrl(href);
  } catch (err) {
    document.dispatchEvent(
      new CustomEvent("parzi:toast", { detail: { text: String(err), err: true } }),
    );
  }
  return true;
}
