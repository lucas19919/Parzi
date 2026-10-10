import { api } from "./api";
import { toast, toastError } from "./toast";
import { openBrainNote } from "./brainStore";
import { fileLink, safeDecode, webLink } from "./linkRules";

export async function handleLinkClick(e: MouseEvent): Promise<boolean> {
  const anchor = (e.target as HTMLElement).closest?.("a");
  if (!anchor) return false;
  e.preventDefault();
  e.stopPropagation();
  const href = anchor.getAttribute("href") ?? "";
  if (!href) return true;

  if (href.startsWith("brain://") || href.startsWith("parzi:note/")) {
    const rawPath = href.replace(/^(brain:\/\/|parzi:note\/)/, "");
    openBrainNote(safeDecode(rawPath).replace(/\\/g, "/"));
    return true;
  }

  if (/^file:/i.test(href)) {
    const link = fileLink(href);
    if (link.kind === "note") {
      openBrainNote(link.path);
    } else if (link.kind === "refuse") {
      toast(link.reason, true);
    } else {
      // The backend asks natively with the full path, then re-checks it.
      await api.openFilePath(link.path).catch(toastError);
    }
    return true;
  }

  if (!/^https?:\/\//i.test(href)) {
    // A bare note path (projects/.../*.md or notes/*.md).
    if (/\.md(#.*)?$/i.test(href)) {
      openBrainNote(safeDecode(href.split("#")[0]).replace(/\\/g, "/"));
    } else {
      toast("That link can't be opened.", true);
    }
    return true;
  }

  const link = webLink(href);
  if (link.kind === "refuse") {
    toast(link.reason, true);
    return true;
  }
  const short = link.url.length > 120 ? `${link.url.slice(0, 120)}…` : link.url;
  if (!window.confirm(`Open in browser?\n${short}`)) return true;
  await api.openConfirmedUrl(link.url).catch(toastError);
  return true;
}
