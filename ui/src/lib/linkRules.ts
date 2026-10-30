
export function safeDecode(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

const RUNS_CODE =
  /\.(exe|bat|cmd|com|ps1|psm1|vbs|vbe|js|jse|wsf|wsh|msi|msp|lnk|scr|hta|jar|reg|cpl|pif|msc|url|scf|application|appref-ms|settingcontent-ms)$/i;

export type FileLink =
  | { kind: "note"; path: string }
  | { kind: "open"; path: string }
  | { kind: "refuse"; reason: string };

const NETWORK = "Network paths don't open from here.";

export function fileLink(href: string): FileLink {
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

  if (/\.md$/i.test(path)) {
    const lower = path.toLowerCase();
    const proj = lower.indexOf("projects/");
    if (proj >= 0) return { kind: "note", path: path.slice(proj) };
    const vault = lower.indexOf("brain/");
    if (vault >= 0) return { kind: "note", path: path.slice(vault + "brain/".length) };
    if (!/^[a-z]:\/|^\//i.test(path)) return { kind: "note", path };
  }

  if (remote || path.startsWith("//")) return { kind: "refuse", reason: NETWORK };
  if (!path.trim() || /[\u0000-\u001f]/.test(path)) return { kind: "refuse", reason: "That file link is broken." };

  const leaf = path.split("/").pop() ?? "";
  const name = leaf.replace(/[. ]+$/, "");
  if (RUNS_CODE.test(name) || (/^[a-z]:\//i.test(path) && leaf.includes(":"))) {
    return { kind: "refuse", reason: "Parzi doesn't open programs or scripts from links." };
  }
  return { kind: "open", path: /^[a-z]:\//i.test(path) ? path.replace(/\//g, "\\") : path };
}

const VAULT_RE = /(?<![A-Za-z0-9_/])(?:(?:projects|notes|widgets)\/[^\s<>\]()`"'*|]+?\.md|brain:\/\/[^\s<>\]()`"'*|]+)(?::\d+(?::\d+)?)?/gi;

const TRAIL = /[.,;:!?)\]}>]+$/;

export interface VaultRef {
  path: string;
  index: number;
  length: number;
}

export function vaultRefs(text: string): VaultRef[] {
  const out: VaultRef[] = [];
  VAULT_RE.lastIndex = 0;
  let m: RegExpExecArray | null;
  let guard = 0;
  while ((m = VAULT_RE.exec(text)) !== null && guard++ < 1000) {
    if (m[0].length === 0) {
      VAULT_RE.lastIndex++;
      continue;
    }
    let path = m[0];
    const trail = TRAIL.exec(path);
    if (trail) path = path.slice(0, -trail[0].length);
    if (!path) continue;
    if (text[m.index - 1] === "`" || text[m.index + path.length] === "`") {
      VAULT_RE.lastIndex = m.index + m[0].length;
      continue;
    }
    out.push({ path, index: m.index, length: path.length });
    VAULT_RE.lastIndex = m.index + path.length;
  }
  VAULT_RE.lastIndex = 0;
  return out;
}

/** A commit SHA, optionally a range (`abc1234..def5678`, `...` works too). */
const SHA_RANGE =
  /(?<![A-Za-z0-9_/])([0-9a-f]{7,40}(?:\.{2,3}[0-9a-f]{7,40})?)(?![A-Za-z0-9_])/gi;

/** A slashed name that reads like a branch (`plan/sidebar-polish`). */
const BRANCH_PATH =
  /(?<![A-Za-z0-9_/.])([A-Za-z0-9._-]{2,}(?:\/[A-Za-z0-9._-]+)+)(?![A-Za-z0-9_/])/gi;

export interface GitRef extends VaultRef {
  kind: "sha" | "branch";
}

/** Commit SHAs, ranges and branch-like `a/b` names in plain prose. */
export function gitRefs(text: string): GitRef[] {
  const out: GitRef[] = [];
  const take = (re: RegExp, kind: GitRef["kind"]) => {
    re.lastIndex = 0;
    let m: RegExpExecArray | null;
    let guard = 0;
    while ((m = re.exec(text)) !== null && guard++ < 1000) {
      if (m[0].length === 0) {
        re.lastIndex++;
        continue;
      }
      let path = m[1];
      const trail = TRAIL.exec(path);
      if (trail) path = path.slice(0, -trail[0].length);
      if (!path) continue;
      if (text[m.index - 1] === "`" || text[m.index + path.length] === "`") {
        re.lastIndex = m.index + m[0].length;
        continue;
      }
      if (kind === "branch" && !isBranchLike(path)) continue;
      out.push({ path, index: m.index, length: path.length, kind });
      re.lastIndex = m.index + path.length;
    }
    re.lastIndex = 0;
  };
  take(SHA_RANGE, "sha");
  take(BRANCH_PATH, "branch");
  out.sort((a, b) => a.index - b.index);
  return out;
}

/**
 * Whether a `a/b/c` path could name a branch (vault notes, dotted file
 * names and single-letter segments excluded). Checked against the repo's
 * real branches on click; anything else falls back to a file open.
 */
export function isBranchLike(path: string): boolean {
  if (!/^[A-Za-z0-9._-]{2,}(?:\/[A-Za-z0-9._-]+)+$/.test(path)) return false;
  if (path.includes("..")) return false;
  if (/\.md$/i.test(path)) return false;
  const leaf = path.slice(path.lastIndexOf("/") + 1);
  if (/\.[A-Za-z][A-Za-z0-9]{0,4}$/.test(leaf)) return false;
  // Plain lowercase words ("either/or", "read/write") are prose, not
  // branches: require a digit, dash, underscore or capital letter.
  if (!/[-_0-9A-Z]/.test(path)) return false;
  return true;
}

export type WebLink = { kind: "open"; url: string } | { kind: "tab"; url: string } | { kind: "refuse"; reason: string };

const LOCAL_HOST = /^(localhost|127(?:\.\d{1,3}){3}|\[::1\]|0\.0\.0\.0|[^/:]+\.localhost)(:\d+)?$/i;
const BACKEND_REJECTS = /[\\"<>`|^$;'()*]/g;
const MAX_LEN = 8192;

export function webLink(href: string): WebLink {
  const m = /^(https?):\/\/([^/?#]*)(.*)$/i.exec(href.trim());
  if (!m) return { kind: "refuse", reason: "That link can't be opened." };
  const host = m[2].replace(/^[^@]*@/, "");
  if (/[\s\u0000-\u001f\u007f]/.test(href.trim())) return { kind: "refuse", reason: "That link can't be opened." };
  if (!m[2]) return { kind: "refuse", reason: "That link can't be opened." };
  if (m[1].toLowerCase() === "http" && LOCAL_HOST.test(host)) {
    const url = `http://${m[2]}${m[3]}`;
    if (url.length > MAX_LEN) return { kind: "refuse", reason: "That link can't be opened." };
    return { kind: "tab", url };
  }
  const url = `https://${m[2]}${m[3]}`.replace(BACKEND_REJECTS, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase().padStart(2, "0")}`);
  if (url.length > MAX_LEN) return { kind: "refuse", reason: "That link can't be opened." };
  return { kind: "open", url };
}
