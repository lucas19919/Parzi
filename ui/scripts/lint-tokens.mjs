import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const root = new URL("../src", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1");
const styled = [];
const code = [];
function walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p);
    else if (/\.(svelte|css)$/.test(name)) styled.push(p);
    else if (/\.ts$/.test(name)) code.push(p);
  }
}
walk(root);

const relOf = (file) => file.replace(/\\/g, "/").split("/src/")[1];
const spaceProp = /(?:^|[;{\s])((?:padding|margin|gap)(?:-(?:top|right|bottom|left|inline|block))?)\s*:\s*([^;]+);/g;
const radiusProp = /(?:^|[;{\s])border-radius\s*:\s*([^;]+);/g;
const sizeProp = /(?:^|[;{\s])font-size\s*:\s*([^;]+);/g;
const weightProp = /(?:^|[;{\s])font-weight\s*:\s*([^;]+);/g;

// Only the switch and the slider track are pills. Everything else is
// --radius at 24px and up, --radius-sm below that, 50% for dots and knobs.
const PILLS = new Set(["lib/settings/shared.css", "lib/settings/Slider.svelte"]);
const WEIGHTS = new Set(["400", "500", "600", "700", "inherit"]);
const RAW_SIZE_OK = new Set(["theme.css"]);

function stylesOf(text, file) {
  if (file.endsWith(".css")) return text;
  return [...text.matchAll(/<style[\s\S]*?<\/style>/g)].map((m) => m[0]).join("\n");
}

const problems = [];
const defined = new Set();
const rootTokens = new Set();
const used = new Map();

for (const file of styled) {
  const rel = relOf(file);
  const text = readFileSync(file, "utf8");
  const css = stylesOf(text, file).replace(/\/\*[\s\S]*?\*\//g, "");
  for (const m of css.matchAll(spaceProp)) {
    if (/px/.test(m[2])) problems.push(`${rel}: ${m[1]}: ${m[2].trim()}`);
  }
  for (const m of css.matchAll(radiusProp)) {
    const rest = m[1]
      .replace(/var\(--(?:radius|radius-sm)\)/g, "")
      .replace(/\b0\b/g, "")
      .replace(/50%/g, "")
      .replace(/inherit/g, "")
      .replace(/!important/g, "")
      .replace(PILLS.has(rel) ? /999px/g : /$^/g, "")
      .trim();
    if (rest) problems.push(`${rel}: border-radius: ${m[1].trim()}`);
  }
  for (const m of css.matchAll(sizeProp)) {
    if (/\d(px|rem)/.test(m[1].replace(/clamp\(.*\)/, "")) && !RAW_SIZE_OK.has(rel)) {
      problems.push(`${rel}: font-size: ${m[1].trim()}`);
    }
  }
  for (const m of css.matchAll(weightProp)) {
    if (!WEIGHTS.has(m[1].trim())) problems.push(`${rel}: font-weight: ${m[1].trim()}`);
  }
  if (rel !== "theme.css" && rel !== "lib/DefaultArt.svelte") {
    for (const m of css.matchAll(/#[0-9a-fA-F]{3,8}\b/g)) {
      problems.push(`${rel}: hex ${m[0]}`);
    }
  }
  if (rel === "theme.css") {
    const block = css.match(/:root\s*\{([\s\S]*?)\n\}/)?.[1] ?? "";
    for (const m of block.matchAll(/^\s*(--[\w-]+)\s*:/gm)) rootTokens.add(m[1]);
  }
  for (const m of text.matchAll(/(--[a-zA-Z][\w-]*)\s*[:=]/g)) defined.add(m[1]);
  for (const m of text.matchAll(/style:(--[\w-]+)/g)) defined.add(m[1]);
  for (const m of text.matchAll(/var\((--[\w-]+)/g)) {
    if (!used.has(m[1])) used.set(m[1], rel);
  }
}

for (const file of code) {
  const text = readFileSync(file, "utf8");
  for (const m of text.matchAll(/["'`](--[a-zA-Z][\w-]*)["'`:]/g)) defined.add(m[1]);
  for (const m of text.matchAll(/var\((--[\w-]+)/g)) {
    if (!used.has(m[1])) used.set(m[1], relOf(file));
  }
  for (const m of text.matchAll(/resolveColor\("(--[\w-]+)"/g)) {
    if (!used.has(m[1])) used.set(m[1], relOf(file));
  }
}

for (const [token, where] of used) {
  if (!defined.has(token)) problems.push(`${where}: var(${token}) is not defined anywhere`);
}
for (const token of rootTokens) {
  if (!used.has(token)) problems.push(`theme.css: ${token} is never used`);
}

if (problems.length) {
  console.error(problems.slice(0, 200).join("\n"));
  if (problems.length > 200) console.error(`... and ${problems.length - 200} more`);
  console.error(`\n${problems.length} token problems`);
  process.exit(1);
}
console.log("tokens ok");
