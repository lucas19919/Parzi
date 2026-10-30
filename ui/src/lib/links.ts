import { api } from "./api";
import { toast, toastError } from "./toast";
import { fileLink, isBranchLike, safeDecode, webLink } from "./linkRules";

async function copyText(text: string, label: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast(`Copied ${label.length > 32 ? `${label.slice(0, 32)}…` : label}`);
  } catch (e) {
    toastError(e);
  }
}

async function openFile(href: string) {
  const link = fileLink(href);
  if (link.kind === "note") {
    pinNote(link.path);
  } else if (link.kind === "refuse") {
    toast(link.reason, true);
  } else {
    await api.openFilePath(link.path).catch(toastError);
  }
}

function pinNote(path: string) {
  const unlined = path.replace(/:\d+(?::\d+)?$/, "");
  document.dispatchEvent(new CustomEvent<string>("parzi:pin-note", { detail: unlined }));
}

export async function handleLinkClick(e: MouseEvent, folder = ""): Promise<boolean> {
  const anchor = (e.target as HTMLElement).closest?.("a");
  if (!anchor) return false;
  e.preventDefault();
  e.stopPropagation();
  const href = anchor.getAttribute("href") ?? "";
  if (!href) return true;

  // A commit SHA, UUID or range chip copies on click: there is nowhere
  // to open it to without a known remote.
  const ref = safeDecode(href.split("#")[0]);
  if (
    /^[0-9a-f]{7,40}(\.{2,3}[0-9a-f]{7,40})?$/i.test(ref) ||
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(ref)
  ) {
    await copyText(ref, ref);
    return true;
  }

  if (href.startsWith("brain://") || href.startsWith("parzi:note/")) {
    const rawPath = href.replace(/^(brain:\/\/|parzi:note\/)/, "");
    pinNote(safeDecode(rawPath).replace(/\\/g, "/"));
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
      pinNote(path);
    } else if (isBranchLike(path)) {
      // A real branch copies; anything else falls through to a file open.
      let branch = false;
      if (folder) {
        const names = await api.gitBranches(folder).then((b) => b.all ?? [], () => [] as string[]);
        branch = names.includes(path) || names.includes(path.replace(/^origin\//, ""));
      }
      if (branch || !folder) {
        await copyText(path, `branch ${path}`);
        return true;
      }
      const file = path.replace(/:\d+(:\d+)?$/, "");
      const full = /^([a-z]:\/|\/)/i.test(file) ? file : `${folder.replace(/[\\/]+$/, "")}/${file.replace(/^\.\//, "")}`;
      await openFile(`file:///${full.replace(/^\//, "")}`);
    } else if (folder && path && !/^[a-z][a-z0-9+.-]*:(?![/\d])/i.test(path)) {
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
  if (link.kind === "tab") {
    document.dispatchEvent(new CustomEvent<string>("parzi:open-page", { detail: link.url }));
    return true;
  }
  document.dispatchEvent(new CustomEvent<string>("parzi:open-page", { detail: link.url }));
  return true;
}
