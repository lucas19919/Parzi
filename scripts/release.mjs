#!/usr/bin/env node
// The version and the release notes, in one place.
//
//   node scripts/release.mjs version          the version now
//   node scripts/release.mjs check            every file agrees on it (CI runs this)
//   node scripts/release.mjs next dev         what the next dev build would be
//   node scripts/release.mjs set 1.5.0        write a version to every file
//   node scripts/release.mjs cut dev          next + set, and for a full release
//                                             move "Unreleased" in CHANGELOG.md
//                                             under the new version
//   node scripts/release.mjs notes 1.5.0      the release page's text
//
// Kinds for next and cut: dev (1.4.1-dev.2 -> 1.4.1-dev.3, 1.4.0 -> 1.4.1-dev.1),
// patch, minor, major (a dev build of that version becomes the release:
// 1.4.1-dev.3 -> 1.4.1), or an exact version.

import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
const at = (p) => join(ROOT, p);
const read = (p) => readFileSync(at(p), "utf8");
const write = (p, s) => writeFileSync(at(p), s);

const SEMVER = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/;

function parse(v) {
  const m = SEMVER.exec(v.replace(/^v/, ""));
  if (!m) throw new Error(`"${v}" is not a version like 1.4.1 or 1.4.1-dev.2`);
  return { major: +m[1], minor: +m[2], patch: +m[3], pre: m[4] ?? "" };
}
const show = ({ major, minor, patch, pre }) => `${major}.${minor}.${patch}${pre ? `-${pre}` : ""}`;

/** Where the version is written, and how each file reads and writes it. */
const FILES = [
  {
    path: "Cargo.toml",
    get: (s) => /\[workspace\.package\][^[]*?\nversion = "([^"]+)"/.exec(s)?.[1],
    set: (s, v) => s.replace(/(\[workspace\.package\][^[]*?\nversion = ")[^"]+(")/, `$1${v}$2`),
  },
  {
    path: "src-tauri/Cargo.toml",
    get: (s) => /\[package\][^[]*?\nversion = "([^"]+)"/.exec(s)?.[1],
    set: (s, v) => s.replace(/(\[package\][^[]*?\nversion = ")[^"]+(")/, `$1${v}$2`),
  },
  ...["src-tauri/tauri.conf.json", "ui/package.json"].map((path) => ({
    path,
    get: (s) => JSON.parse(s).version,
    set: (s, v) => s.replace(/("version":\s*")[^"]+(")/, `$1${v}$2`),
  })),
  {
    path: "ui/package-lock.json",
    get: (s) => {
      const j = JSON.parse(s);
      return j.version === j.packages?.[""]?.version ? j.version : `${j.version} / ${j.packages?.[""]?.version}`;
    },
    set: (s, v) => {
      const j = JSON.parse(s);
      j.version = v;
      if (j.packages?.[""]) j.packages[""].version = v;
      return `${JSON.stringify(j, null, 2)}\n`;
    },
  },
  ...["Cargo.lock", "src-tauri/Cargo.lock"].map((path) => {
    const ours = /(name = "parzi-(?:core|providers|runtime|cli|app)"\nversion = ")([^"]+)(")/g;
    return {
      path,
      get: (s) => {
        const all = [...new Set([...s.matchAll(ours)].map((m) => m[2]))];
        return all.length === 1 ? all[0] : all.join(" / ") || undefined;
      },
      set: (s, v) => s.replace(ours, `$1${v}$3`),
    };
  }),
];

function current() {
  const v = FILES[0].get(read(FILES[0].path));
  if (!v) throw new Error("Cargo.toml has no [workspace.package] version");
  return v;
}

function check() {
  const want = current();
  const off = FILES.map((f) => [f.path, f.get(read(f.path))]).filter(([, v]) => v !== want);
  if (off.length) {
    throw new Error(`Cargo.toml says ${want}, but ${off.map(([p, v]) => `${p} says ${v ?? "nothing"}`).join("; ")}. Fix with: node scripts/release.mjs set ${want}`);
  }
  return want;
}

function set(v) {
  const clean = show(parse(v));
  for (const f of FILES) {
    const before = read(f.path);
    const after = f.set(before, clean);
    if (f.get(after) !== clean) throw new Error(`could not write the version into ${f.path}`);
    write(f.path, after);
  }
  return clean;
}

function next(kind, from = current()) {
  const v = parse(from);
  const dev = /^dev\.(\d+)$/.exec(v.pre);
  switch (kind) {
    case "dev":
      if (dev) return show({ ...v, pre: `dev.${+dev[1] + 1}` });
      if (v.pre) throw new Error(`can't tell the next dev build after ${from}`);
      return show({ ...v, patch: v.patch + 1, pre: "dev.1" });
    case "patch":
      return show(v.pre ? { ...v, pre: "" } : { ...v, patch: v.patch + 1 });
    case "minor":
      return show(v.pre && v.patch === 0 ? { ...v, pre: "" } : { ...v, minor: v.minor + 1, patch: 0, pre: "" });
    case "major":
      return show(v.pre && v.patch === 0 && v.minor === 0 ? { ...v, pre: "" } : { major: v.major + 1, minor: 0, patch: 0, pre: "" });
    default:
      return show(parse(kind));
  }
}

const CHANGELOG = "CHANGELOG.md";
const UNRELEASED = /^## Unreleased[^\n]*\n([\s\S]*?)(?=^## |(?![\s\S]))/m;

/** The changelog's section for a version, or Unreleased for a dev build. */
function section(v) {
  const log = read(CHANGELOG);
  const esc = v.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const own = new RegExp(`^## v?${esc}(?:\\s[^\\n]*)?\\n([\\s\\S]*?)(?=^## |(?![\\s\\S]))`, "m").exec(log);
  const body = (own ?? UNRELEASED.exec(log))?.[1].trim();
  if (!body) throw new Error(`${CHANGELOG} has nothing under "## Unreleased" or "## ${v}": say what changed first`);
  return body;
}

/** A full release takes the Unreleased notes as its own and starts a new list. */
function moveUnreleased(v) {
  const log = read(CHANGELOG);
  const m = UNRELEASED.exec(log);
  if (!m || !m[1].trim()) throw new Error(`${CHANGELOG} has nothing under "## Unreleased": say what changed first`);
  const date = new Date().toISOString().slice(0, 10);
  write(CHANGELOG, log.replace(UNRELEASED, `## Unreleased\n\n## ${v} (${date})\n\n${m[1].trim()}\n\n`));
}

function notes(v) {
  const clean = show(parse(v));
  const pre = parse(clean).pre;
  const lead = pre ? `A dev build: a preview of what is coming next. Turn on Settings, General, Dev builds to get these as they come out.` : "";
  return [
    `## Parzi v${clean}`,
    "",
    "One harness, six providers: Claude, Codex, Antigravity, OpenCode, Grok, Cursor.",
    ...(lead ? ["", lead] : []),
    "",
    "### What changed",
    "",
    section(clean),
    "",
    read(".github/release-footer.md").trim(),
    "",
  ].join("\n");
}

const run = process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href;
const [cmd, arg] = run ? process.argv.slice(2) : [];
if (run) try {
  switch (cmd) {
    case "version":
      console.log(current());
      break;
    case "check":
      console.log(`version ${check()} everywhere`);
      break;
    case "next":
      console.log(next(arg ?? "dev"));
      break;
    case "set":
      if (!arg) throw new Error("set needs a version");
      console.log(set(arg));
      break;
    case "cut": {
      check();
      const v = next(arg ?? "dev");
      section(v);
      if (!parse(v).pre) moveUnreleased(v);
      console.log(set(v));
      break;
    }
    case "notes":
      process.stdout.write(notes(arg ?? current()));
      break;
    default:
      console.log(read("scripts/release.mjs").split("\n").filter((l) => l.startsWith("//")).map((l) => l.slice(3)).join("\n"));
  }
} catch (e) {
  console.error(`release: ${e.message}`);
  process.exit(1);
}

export { next, parse };
