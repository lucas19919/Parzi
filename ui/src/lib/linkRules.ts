export function safeDecode(s: string): string {
  try {
    return decodeURIComponent(s);
  } catch {
    return s;
  }
}

/** `dir/../a/./b.md` → `a/b.md`, keeping a leading drive or slash. */
export function joinPath(dir: string, rel: string): string {
  const out: string[] = [];
  `${dir}/${rel}`
    .replace(/\\/g, "/")
    .split("/")
    .forEach((seg, i) => {
      if (seg === "." || (seg === "" && i > 0)) return;
      if (seg !== "..") return void out.push(seg);
      const last = out[out.length - 1];
      // Never climb above a drive or the filesystem root.
      if (out.length && last !== "" && !/^[a-z]:$/i.test(last)) out.pop();
    });
  return out.join("/");
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

const VAULT_RE = /(?<![A-Za-z0-9_/\-.])(?:(?:projects|notes|widgets)\/[^\s<>\]()`"'*|]+?\.md|brain:\/\/[^\s<>\]()`"'*|]+)(?::\d+(?::\d+)?)?/gi;

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

/** A commit SHA, optionally a range (`abc1234..def5678`, `...` works too). Dashes excluded so UUID segments never match. */
const SHA_RANGE =
  /(?<![A-Za-z0-9_/\-])([0-9a-f]{7,40}(?:\.{2,3}[0-9a-f]{7,40})?)(?![A-Za-z0-9_\-])/gi;

/** A full UUID (`123e4567-e89b-12d3-a456-426614174000`): copies on click like a SHA. */
const UUID_RE =
  /(?<![A-Za-z0-9_/\-])([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})(?![A-Za-z0-9_\-])/gi;

/** A slashed name that reads like a branch (`plan/sidebar-polish`). */
const BRANCH_PATH =
  /(?<![A-Za-z0-9_/.\-])([A-Za-z0-9._-]{2,}(?:\/[A-Za-z0-9._-]+)+)(?![A-Za-z0-9_/])/gi;

export interface GitRef extends VaultRef {
  kind: "sha" | "branch";
}

/** Commit SHAs, UUIDs, ranges and branch-like `a/b` names in plain prose. */
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
      const at = m.index + m[0].indexOf(m[1]);
      if (text[at - 1] === "`" || text[at + path.length] === "`") {
        re.lastIndex = m.index + m[0].length;
        continue;
      }
      if (kind === "branch" && !isBranchLike(path)) continue;
      out.push({ path, index: at, length: path.length, kind });
      re.lastIndex = at + path.length;
    }
    re.lastIndex = 0;
  };
  take(UUID_RE, "sha");
  take(SHA_RANGE, "sha");
  take(BRANCH_PATH, "branch");
  out.sort((a, b) => a.index - b.index || b.length - a.length);
  // Continuation-safe: keep the longest ref at each position, drop overlaps
  // so a UUID/SHA never double-links its own segments and a SHA inside a
  // note path stays one link (vault wins ties in md.ts).
  return out.filter((r, i, all) => !all.slice(0, i).some((p) => r.index < p.index + p.length));
}

export interface WikiRef {
  path: string;
  label: string;
  index: number;
  length: number;
}

const WIKI_RE = /\[\[([^\]\n|`]+?)(?:\|([^\]\n`]+?))?\]\]/g;

/** Obsidian/Parzi `[[note]]` and `[[projects/foo/bar.md|Alias]]` wikilinks. */
export function wikiRefs(text: string): WikiRef[] {
  const out: WikiRef[] = [];
  WIKI_RE.lastIndex = 0;
  let m: RegExpExecArray | null;
  let guard = 0;
  while ((m = WIKI_RE.exec(text)) !== null && guard++ < 1000) {
    if (m[0].length === 0) {
      WIKI_RE.lastIndex++;
      continue;
    }
    const rawTarget = m[1].trim();
    if (!rawTarget) continue;
    const at = m.index;
    if (text[at - 1] === "`" || text[at + m[0].length] === "`") {
      WIKI_RE.lastIndex = at + m[0].length;
      continue;
    }
    const [targetPart, anchor] = rawTarget.split("#", 2);
    const normalized = /\.[a-z0-9]+$/i.test(targetPart) ? targetPart : `${targetPart}.md`;
    const href = `brain://${normalized.replace(/\\/g, "/")}${anchor ? `#${anchor}` : ""}`;
    const alias = m[2]?.trim();
    const label = alias || chipLabel(normalized.replace(/\\/g, "/"), true);
    out.push({ path: href, label, index: at, length: m[0].length });
    WIKI_RE.lastIndex = at + m[0].length;
  }
  WIKI_RE.lastIndex = 0;
  return out;
}

const FILE_EXT =
  "rs|ts|tsx|mts|cts|js|jsx|mjs|cjs|svelte|vue|py|go|c|h|cc|cpp|cxx|hpp|cs|java|kt|swift|rb|php|css|scss|less|html|htm|xml|svg|json|jsonc|toml|yaml|yml|md|mdx|txt|sql|sh|bash|zsh|ps1|lock|ini|cfg|conf|env|graphql|proto|tex|csv|pdf|png|jpg|jpeg|webp|gif";

const FILE_PATH_RE = new RegExp(
  `(?<![A-Za-z0-9_@:/\\\\.-])((?:[A-Za-z]:[\\\\/]|(?:\\.\\.?|[A-Za-z0-9_.@-]+)[\\\\/])(?:[A-Za-z0-9_.@-]+[\\\\/])*[A-Za-z0-9_.@-]+\\.(?:${FILE_EXT})(?::\\d+(?:[-:]\\d+)?)?(?:#L\\d+(?:-L?\\d+)?)?)(?![A-Za-z0-9_/-]|\\.[A-Za-z0-9])`,
  "gi",
);

const ROOT_FILE_RE =
  /(?<![A-Za-z0-9_@:/\\.-])((?:Cargo\.toml|Cargo\.lock|package\.json|tsconfig\.json|README\.md|CHANGELOG\.md|LICENSE\.md|CLAUDE\.md|AGENTS\.md|vite\.config\.[cm]?[jt]s|svelte\.config\.[cm]?[jt]s|tauri\.conf\.json)(?::\d+(?:[-:]\d+)?)?(?:#L\d+(?:-L?\d+)?)?)(?![A-Za-z0-9_/-]|\.[A-Za-z0-9])/g;

/** Normalizes a workspace or absolute file ref into a DOMPurify-safe href. */
export function fileRefHref(path: string): string {
  const clean = path.replace(/\\/g, "/");
  if (/^[A-Za-z]:\//.test(clean)) return `file:///${clean}`;
  if (clean.startsWith("/")) return `file://${clean}`;
  if (!clean.includes("/") && clean.includes(":")) return `./${clean}`;
  return clean;
}

/** Workspace-relative or absolute source file paths (and known root configs) in prose. */
export function fileRefs(text: string): VaultRef[] {
  const out: VaultRef[] = [];
  const take = (re: RegExp) => {
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
      const at = m.index + m[0].indexOf(m[1]);
      if (text[at - 1] === "`" || text[at + path.length] === "`") {
        re.lastIndex = m.index + m[0].length;
        continue;
      }
      out.push({ path, index: at, length: path.length });
      re.lastIndex = at + path.length;
    }
    re.lastIndex = 0;
  };
  take(FILE_PATH_RE);
  take(ROOT_FILE_RE);
  out.sort((a, b) => a.index - b.index || b.length - a.length);
  return out.filter((r, i, all) => !all.slice(0, i).some((p) => r.index < p.index + p.length));
}

/** True when the whole trimmed line is exactly one linkable ref. */
export function wholeRef(line: string): { path: string; vault: boolean; label?: string } | null {
  const t = line.trim().replace(/^`+|`+$/g, "").trim();
  if (!t) return null;
  const w = wikiRefs(t);
  if (w.length === 1 && w[0].index === 0 && w[0].length === t.length) {
    return { path: w[0].path, vault: true, label: w[0].label };
  }
  if (/\s/.test(t)) return null;
  const v = vaultRefs(t);
  if (v.length === 1 && v[0].index === 0 && v[0].length === t.length) return { path: v[0].path, vault: true };
  const f = fileRefs(t);
  if (f.length === 1 && f[0].index === 0 && f[0].length === t.length) {
    return { path: fileRefHref(f[0].path), vault: false, label: f[0].path };
  }
  const g = gitRefs(t);
  if (g.length === 1 && g[0].index === 0 && g[0].length === t.length) return { path: g[0].path, vault: false };
  return null;
}

/** Chip label: vault notes show just the file name, everything else full. */
export function chipLabel(path: string, vault: boolean): string {
  if (!vault) return path;
  return path.split("/").pop() ?? path;
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
  const bare = host.replace(/:\d+$/, "");
  const nonPublic = LOCAL_HOST.test(host) || /^(localhost|127(?:\.\d{1,3}){3}|\[::1\]|0\.0\.0\.0|[^/:.]+|[^/:]+\.(localhost|local|lan|internal|intranet|home|corp|localdomain))$/i.test(bare);
  if (m[1].toLowerCase() === "http" && nonPublic) {
    const url = `http://${m[2]}${m[3]}`;
    if (url.length > MAX_LEN) return { kind: "refuse", reason: "That link can't be opened." };
    return { kind: "tab", url };
  }
  const url = `https://${m[2]}${m[3]}`.replace(BACKEND_REJECTS, (c) => `%${c.charCodeAt(0).toString(16).toUpperCase().padStart(2, "0")}`);
  if (url.length > MAX_LEN) return { kind: "refuse", reason: "That link can't be opened." };
  return { kind: "open", url };
}


/** Schemeless web addresses linkify-it never sees: localhost, bare IPs and
 * dotted hosts with an unknown TLD, but only with a port, path, query or
 * fragment. A bare name without those is a filename until proven otherwise
 * (README.md), and host:port alone is a file:line ref (md.ts:435). */
const BARE_WEB =
  /(^|[\s(,;>\["“”<=])(localhost|(?:\d{1,3}\.){3}\d{1,3}|[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+)(:\d{1,5})?([/?#][^\s<>]*)?/g;
const BARE_TRAIL = /[.,;:!?)\]}>"*`|]/;

function bareHostOk(host: string): boolean {
  if (/^localhost$/i.test(host)) return true;
  const ip = /^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(host);
  if (ip) return ip.slice(1).every((o) => Number(o) <= 255);
  return host.slice(host.lastIndexOf(".") + 1).length >= 2;
}

export interface BareWebRef {
  href: string;
  index: number;
  length: number;
}

export function bareWebRefs(text: string): BareWebRef[] {
  const out: BareWebRef[] = [];
  BARE_WEB.lastIndex = 0;
  let m: RegExpExecArray | null;
  let guard = 0;
  while ((m = BARE_WEB.exec(text)) !== null && guard++ < 1000) {
    const at = m.index + m[1].length;
    const host = m[2];
    const rest = `${m[3] ?? ""}${m[4] ?? ""}`;
    if (!rest || !bareHostOk(host)) continue;
    if (!/[/?#]/.test(rest) && !/^(localhost|(?:\d{1,3}\.){3}\d{1,3})$/.test(host)) continue;
    let shown = host + rest;
    while (shown.length > host.length && BARE_TRAIL.test(shown[shown.length - 1])) {
      if (shown.endsWith(")")) {
        const opens = shown.split("(").length - 1;
        const closes = shown.split(")").length - 1;
        if (closes <= opens) break;
      }
      shown = shown.slice(0, -1);
    }
    if (shown.length <= host.length) continue;
    out.push({ href: `http://${shown}`, index: at, length: shown.length });
    BARE_WEB.lastIndex = at + shown.length;
  }
  BARE_WEB.lastIndex = 0;
  return out;
}