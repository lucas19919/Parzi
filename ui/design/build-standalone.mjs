import { readFileSync, writeFileSync } from "node:fs";
const img = "data:image/jpeg;base64," + readFileSync("asuka.jpg").toString("base64");
const boards = [
  ["Home", "Main.dc.html", 1440, 900],
  ["Thread", "Thread.dc.html", 1440, 900],
  ["Composer, model menu open", "Composer.dc.html", 880, 620],
];
let styles = new Set(), fonts = new Set(), sections = "";
for (const [title, file, w, h] of boards) {
  const src = readFileSync(file, "utf8");
  const helmet = src.match(/<helmet>([\s\S]*?)<\/helmet>/)[1];
  for (const m of helmet.matchAll(/<link[^>]+>/g)) fonts.add(m[0]);
  const css = helmet.match(/<style>([\s\S]*?)<\/style>/)[1].replace(/body \{[^}]*\}/, "");
  const body = src.split("</helmet>")[1].split("</x-dc>")[0].replaceAll('src="asuka.jpg"', `src="${img}"`);
  styles.add(css);
  sections += `<section class="board"><h2>${title} <span>${w} × ${h}</span></h2><div class="frame" style="width:${w}px;height:${h}px">${body}</div></section>\n`;
}
const html = `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Parzi Redesign</title>
${[...fonts].join("\n")}
<style>
  body { margin: 0; background: #050507; color: #ededf2; font-family: Inter, system-ui, sans-serif; padding: 48px 40px 80px; }
  h1 { font-family: 'Instrument Serif', Georgia, serif; font-style: italic; font-weight: 400; font-size: 40px; margin: 0 0 6px; }
  .lede { color: #8d93a1; font-size: 14px; margin: 0 0 40px; }
  .board { margin-bottom: 56px; }
  .board h2 { font-size: 13px; font-weight: 500; color: #c9cdd8; margin: 0 0 12px; letter-spacing: 0.01em; }
  .board h2 span { color: #6f7583; font-family: 'JetBrains Mono', ui-monospace, monospace; font-size: 11px; margin-left: 8px; }
  .frame { border-radius: 12px; overflow: hidden; box-shadow: 0 30px 80px rgba(0,0,0,0.7), 0 0 0 1px rgba(255,255,255,0.06); max-width: 100%; }
  .wrap { overflow-x: auto; padding-bottom: 8px; }
  ${[...styles].join("\n")}
</style></head>
<body>
<h1>Parzi</h1>
<p class="lede">Late night glass. Home, thread, and the composer with the model menu open.</p>
<div class="wrap">
${sections}
</div>
</body></html>`;
writeFileSync("parzi-redesign-mockups.html", html);
console.log("wrote parzi-redesign-mockups.html", (html.length / 1024).toFixed(0) + " KB");
