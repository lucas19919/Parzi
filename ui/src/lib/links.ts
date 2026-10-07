import { api } from "./api";
import { toast, toastError } from "./toast";
import { openBrainNote } from "./brainStore";

export async function handleLinkClick(e: MouseEvent): Promise<boolean> {
  const anchor = (e.target as HTMLElement).closest?.("a");
  if (!anchor) return false;
  e.preventDefault();
  e.stopPropagation();
  const href = anchor.getAttribute("href") ?? "";
  if (!href) return true;

  if (href.startsWith("brain://") || href.startsWith("parzi:note/")) {
    const rawPath = href.replace(/^(brain:\/\/|parzi:note\/)/, "");
    openBrainNote(decodeURIComponent(rawPath).replace(/\\/g, "/"));
    return true;
  }

  if (href.startsWith("file://")) {
    let filePath = href.slice("file://".length).replace(/\\/g, "/");
    // On Windows, strip leading slash for drive letters e.g. /C:/path -> C:/path
    if (/^\/[a-zA-Z]:/.test(filePath)) {
      filePath = filePath.slice(1);
    }
    // Strip trailing line hashes e.g. #L10-L20
    const cleanPath = decodeURIComponent(filePath.split("#")[0]);
    if (/\.md$/i.test(cleanPath)) {
      const projIdx = cleanPath.toLowerCase().indexOf("projects/");
      if (projIdx >= 0) {
        openBrainNote(cleanPath.slice(projIdx));
        return true;
      }
      const brainIdx = cleanPath.toLowerCase().indexOf("brain/");
      if (brainIdx >= 0) {
        openBrainNote(cleanPath.slice(brainIdx + "brain/".length));
        return true;
      }
    }
    if (/\.md$/i.test(cleanPath)) {
      // Bare .md file path outside the vault: open in brain if relative,
      // else fall through to the OS handler.
      if (!/^[a-zA-Z]:[\/\\]|^\//.test(cleanPath)) {
        openBrainNote(cleanPath);
        return true;
      }
    }
    await api.openFilePath(cleanPath).catch(toastError);
    return true;
  }

  if (!/^https?:\/\//i.test(href)) {
    // If it looks like a note path (e.g. projects/.../*.md or notes/*.md)
    if (/\.md(#.*)?$/i.test(href)) {
      const clean = href.split("#")[0].replace(/\\/g, "/");
      openBrainNote(decodeURIComponent(clean));
      return true;
    }
    return true;
  }

  const short = href.length > 120 ? `${href.slice(0, 120)}…` : href;
  if (!window.confirm(`Open in browser?\n${short}`)) return true;
  await api.openConfirmedUrl(href).catch(toastError);
  return true;
}
