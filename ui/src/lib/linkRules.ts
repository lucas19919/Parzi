// Pure link rules behind links.ts (no imports, so node tests load it).
// Every outbound open is decided here first, so nothing reaches a
// confirm() that the backend would then refuse.

export function safeDecode(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

// Files that run code when the OS "opens" them. Refused outright.
const RUNS_CODE =
  /\.(exe|bat|cmd|com|ps1|psm1|vbs|vbe|js|jse|wsf|wsh|msi|msp|lnk|scr|hta|jar|reg|cpl|pif|msc|url|scf|application|appref-ms|settingcontent-ms)$/i;

export type FileLink =
  | { kind: "note"; path: string }
  | { kind: "open"; path: string }
  | { kind: "refuse"; reason: string };

const NETWORK = "Network paths don't open from here.";

export function fileLink(href: string): FileLink {
  // Query and line anchors (#L10-L20) are not part of the path.
  let rest = href.replace(/^file:/i, "").split(/[?#]/)[0];
  let remote = false;
  if (rest.startsWith("//")) {
    const auth = rest.slice(2);
    const slash = auth.indexOf("/");
    const host = (slash < 0 ? auth : auth.slice(0, slash)).toLowerCase();
    if (auth.startsWith("/") || /^[a-z]:/i.test(auth)) rest = auth;
    else if (host === "localhost") rest = slash < 0 ? "/" : auth.slice(slash);
    else {
      remote = true;
      rest = auth;
    }
  }
  let path = safeDecode(rest).replace(/\\/g, "/");
  if (/^\/[a-z]:/i.test(path)) path = path.slice(1);

  // Notes open inside Parzi (a brain read), never through the OS.
  if (/\.md$/i.test(path)) {
    const lower = path.toLowerCase();
    const proj = lower.indexOf("projects/");
    if (proj >= 0) return { kind: "note", path: path.slice(proj) };
    const vault = lower.indexOf("brain/");
    if (vault >= 0) return { kind: "note", path: path.slice(vault + "brain/".length) };
    if (!/^[a-z]:\/|^\//i.test(path)) return { kind: "note", path };
  }

  // UNC (\\host\share), device (\\?\, \\.\), file://host and file://// forms.
  if (remote || path.startsWith("//")) return { kind: "refuse", reason: NETWORK };
  if (!path.trim() || /[\u0000-\u001f]/.test(path)) return { kind: "refuse", reason: "That file link is broken." };

  const leaf = path.split("/").pop() ?? "";
  // Windows drops trailing dots and spaces ("run.exe. " runs run.exe);
  // a colon in the name is an alternate data stream.
  const name = leaf.replace(/[. ]+$/, "");
  if (RUNS_CODE.test(name) || (/^[a-z]:\//i.test(path) && leaf.includes(":"))) {
    return { kind: "refuse", reason: "Parzi doesn't open programs or scripts from links." };
  }
  return { kind: "open", path: /^[a-z]:\//i.test(path) ? path.replace(/\//g, "\\") : path };
}

export type WebLink = { kind: "open"; url: string } | { kind: "tab"; url: string } | { kind: "refuse"; reason: string };

const LOCAL_HOST = /^(localhost|127(?:\.\d{1,3}){3}|\[::1\]|0\.0\.0\.0|[^/:]+\.localhost)(:\d+)?$/i;
// Characters the backend rejects (crates/parzi-core/src/urls.rs); the
// same bytes percent-encoded are equivalent for real-world URLs.
const BACKEND_REJECTS = /[\\"<>`|^$;'()*]/g;
const MAX_LEN = 8192;

export function webLink(href: string): WebLink {
  const m = /^(https?):\/\/([^/?#]*)(.*)$/i.exec(href.trim());
  if (!m) return { kind: "refuse", reason: "That link can't be opened." };
  const host = m[2].replace(/^[^@]*@/, "");
  if (/[\s\u0000-\u001f\u007f]/.test(href.trim())) return { kind: "refuse", reason: "That link can't be opened." };
  if (!m[2]) return { kind: "refuse", reason: "That link can't be opened." };
  // Local pages (an agent's preview, a dev server) open in a Parzi tab:
  // the tab reuses the agent's open page when it has the same URL.
  // The scheme stays http: forcing https would break plain local servers.
  if (m[1].toLowerCase() === "http" && LOCAL_HOST.test(host)) {
    const url = `http://${m[2]}${m[3]}`;
    if (url.length > MAX_LEN) return { kind: "refuse", reason: "That link can't be opened." };
    return { kind: "tab", url };
  }
  const url = `https://${m[2]}${m[3]}`.replace(BACKEND_REJECTS, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase().padStart(2, "0")}`);
  if (url.length > MAX_LEN) return { kind: "refuse", reason: "That link can't be opened." };
  return { kind: "open", url };
}
