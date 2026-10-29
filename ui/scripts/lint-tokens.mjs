import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const root = new URL("../src", import.meta.url);
const files = [];
function walk(dir) {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) walk(p);
    else if (/\.(svelte|css)$/.test(name)) files.push(p);
  }
}
walk(root.pathname.replace(/^\/([A-Za-z]:)/, "$1"));

const spaceProp = /(?:^|[;{\s])((?:padding|margin|gap)(?:-(?:top|right|bottom|left|inline|block))?)\s*:\s*([^;]+);/g;
const radiusProp = /(?:^|[;{\s])(border-radius)\s*:\s*([^;]+);/g;

function stylesOf(text, file) {
  if (file.endsWith(".css")) return text;
  return [...text.matchAll(/<style[\s\S]*?<\/style>/g)].map((m) => m[0]).join("\n");
}

const problems = [];
for (const file of files) {
  const rel = file.replace(/\\/g, "/").split("/src/")[1];
  const css = stylesOf(readFileSync(file, "utf8"), file).replace(/\/\*[\s\S]*?\*\//g, "");
  const inTheme = rel === "theme.css";
  for (const re of [spaceProp, radiusProp]) {
    re.lastIndex = 0;
    for (const m of css.matchAll(re)) {
      const value = m[2];
      if (re === radiusProp) {
        const rest = value
          .replace(/var\(--(?:r|radius|radius-sm|radius-lg)\)/g, "0")
          .replace(/calc\([^)]*\)/g, "0")
          .replace(/\b0\b/g, "")
          .replace(/999px/g, "")
          .replace(/50%/g, "")
          .replace(/25%/g, "")
          .replace(/!important/g, "")
          .trim();
        if (/\d/.test(rest)) problems.push(`${rel}: border-radius: ${value.trim()}`);
      } else if (/px/.test(value)) {
        problems.push(`${rel}: ${m[1]}: ${value.trim()}`);
      }
    }
  }
  if (!inTheme && rel !== "lib/DefaultArt.svelte") {
    for (const m of css.matchAll(/#[0-9a-fA-F]{3,8}\b/g)) {
      problems.push(`${rel}: hex ${m[0]}`);
    }
  }
}

if (problems.length) {
  console.error(problems.slice(0, 200).join("\n"));
  if (problems.length > 200) console.error(`... and ${problems.length - 200} more`);
  console.error(`\n${problems.length} token problems`);
  process.exit(1);
}
console.log("tokens ok");
