import { api } from "./api";
import { toast, toastError } from "./toast";
import { openBrainNote } from "./brainStore";
import { fileLink, safeDecode, webLink } from "./linkRules";

async function openFile(href: string) {
  const link = fileLink(href);
  if (link.kind === "note") {
    openBrainNote(link.path);
  } else if (link.kind === "refuse") {
    toast(link.reason, true);
  } else {
    // The backend asks natively with the full path, then re-checks it.
    await api.openFilePath(link.path).catch(toastError);
  }
}

// `folder` is the session's folder: agents link code by its path there
// ("ui/src/App.svelte", "./README.md:12").
export async function handleLinkClick(e: MouseEvent, folder = ""): Promise<boolean> {
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
    await openFile(href);
    return true;
  }

  if (!/^https?:\/\//i.test(href)) {
    const path = safeDecode(href.split("#")[0]).replace(/\\/g, "/");
    const note = /^(projects|notes|widgets)\//i.test(path) || (!folder && /\.md$/i.test(path));
    if (note) {
      openBrainNote(path);
    } else if (folder && path && !/^[a-z][a-z0-9+.-]*:(?![/\d])/i.test(path)) {
      // A trailing ":12" or ":12:3" names a line, not part of the file.
      const file = path.replace(/:\d+(:\d+)?$/, "");
      const full = /^([a-z]:\/|\/)/i.test(file) ? file : `${folder.replace(/[\\/]+$/, "")}/${file.replace(/^\.\//, "")}`;
      await openFile(`file:///${full.replace(/^\//, "")}`);
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
  // A local page opens as a Parzi tab: an agent's preview or dev server
  // lands next to the chat, reusing its tab when one shows the same URL.
  if (link.kind === "tab") {
    document.dispatchEvent(new CustomEvent<string>("parzi:open-page", { detail: link.url }));
    return true;
  }
  // Every web link opens in a Parzi tab, never in the system browser: the
  // page toolbar keeps an explicit "open outside" button for the rare case.
  document.dispatchEvent(new CustomEvent<string>("parzi:open-page", { detail: link.url }));
  return true;
}
