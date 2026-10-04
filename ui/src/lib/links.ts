import { api } from "./api";
import { toastError } from "./toast";

export async function handleLinkClick(e: MouseEvent): Promise<boolean> {
  const anchor = (e.target as HTMLElement).closest?.("a");
  if (!anchor) return false;
  e.preventDefault();
  e.stopPropagation();
  const href = anchor.getAttribute("href") ?? "";
  if (!/^https:\/\//i.test(href)) return true;
  const short = href.length > 120 ? `${href.slice(0, 120)}…` : href;
  if (!window.confirm(`Open in browser?\n${short}`)) return true;
  await api.openConfirmedUrl(href).catch(toastError);
  return true;
}
