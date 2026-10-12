// Runs lintapple (ui-policy.json) over every main view of the app, served
// by `npm run dev` with the mock backend (mock.html). Exit 1 on findings.
//   npm run dev            (in another terminal)
//   npm run lint:ui        [--json report.json] [view ...]
import { readFile, writeFile } from "node:fs/promises";
import { checkUrl } from "lintapple";

const BASE = process.env.PARZI_UI_URL ?? "http://localhost:1420/mock.html";
const VIEWS = ["home", "session", "brain", "history", "work", "settings", "setup", "page"];

// More states per view, each checked again after its action (run in order
// on one page). Selectors are CSS and must match exactly one element.
const click = (name, selector, ready) => ({ name, action: "click", selector, ready: { selector: ready } });
const nth = (list, i) => `${list} > :nth-child(${i})`;
const STATES = {
  home: [
    click("project menu", ".proj-out", ".menu-pop"),
    // Pressed in the composer: the audit may already have scrolled the menu shut.
    { name: "project menu closed", action: "press", selector: ".ob textarea", key: "Escape", ready: { selector: ".menu-pop", state: "hidden" } },
    // The audit scrolls, which closes popovers: the next state starts from the page.
    click("customize home", ".customize", ".home-menu"),
    { name: "customize closed", action: "press", selector: ".ob textarea", key: "Escape", ready: { selector: ".home-menu", state: "hidden" } },
    click("search mode", ".modes > button:first-child", ".box.web"),
    { name: "web suggestions", action: "fill", selector: ".ob textarea", value: "parzi docs", ready: { selector: ".web-suggest" } },
  ],
  session: [
    { name: "switcher", action: "press", selector: ".ob textarea", key: "Control+p", ready: { selector: '.box[role="dialog"]' } },
    { name: "switcher closed", action: "press", selector: '.box[role="dialog"] input', key: "Escape", ready: { selector: '.box[role="dialog"]', state: "hidden" } },
    click("model picker", ".bar .model", ".picker"),
    click("permission menu", ".perm-btn", ".menu-pop.perms"),
    click("session actions", '.session .menu button[aria-haspopup="menu"]', '.dropdown[role="menu"]'),
    click("context usage", ".icon-btn.ctx", ".ctx-pop"),
    { name: "context closed", action: "press", selector: ".ob textarea", key: "Escape", ready: { selector: ".ctx-pop", state: "hidden" } },
    click("dock", '.session button[aria-pressed][aria-label^="Toggle dock"]', ".panel .p-tabs"),
    // The open session leads a goal (the strip above the thread); this is a
    // step's session, opened from the dock.
    click("step session", '.panel .row[style] .open[title="Find what runs on every keystroke"]', ".strip.child"),
    click("dock work", ".panel .p-tabs > button:nth-child(3)", ".panel .work-dock"),
    click("goal from strip", ".strip .open", ".goal-page"),
  ],
  brain: [click("pinned note", ".note:has(.pin)", ".titles h2"), click("conflict copy", ".note.copy", ".conflict")],
  work: [
    click("goal page", '[data-goal="g-plan"]', '.goal-page [aria-labelledby="g-plan"]'),
    { name: "follow-up typed", action: "fill", selector: ".follow input", value: "Keep the old autosize as a fallback", ready: { selector: ".follow .send:not(:disabled)" } },
    click("finished goal", '[data-goal="g-done"]', ".goal-page .files"),
    click("goal needing you", '[data-goal="g-check"]', ".goal-page .checks"),
    { name: "project filter", action: "press", selector: ".bar .chip select", key: "ArrowDown", ready: { selector: '.work:not([data-project="*"])' } },
    click("engine filter", ".engines > button:nth-child(3)", '.work[data-engine="server"]'),
  ],
  history: [2, 3, 4].map((i) => click(`library ${i}`, nth(".seg", i), `${nth(".seg", i)}[aria-pressed="true"]`)),
  settings: [2, 3, 4, 5].map((i) => click(`section ${i}`, nth(".sections", i), `${nth(".sections", i)}[aria-selected="true"]`)),
  setup: [2, 3, 4, 5].map((i) => click(`step ${i}`, nth("header .steps", i), `${nth("header .steps", i)}[aria-current="step"]`)),
};

const args = process.argv.slice(2);
const jsonAt = args.indexOf("--json");
const jsonPath = jsonAt >= 0 ? args.splice(jsonAt, 2)[1] : "";
const views = args.length ? args : VIEWS;

const policy = JSON.parse(await readFile(new URL("../ui-policy.json", import.meta.url), "utf8"));
const all = [];
let failed = false;
for (const view of views) {
  const interactions = STATES[view] ? { states: STATES[view] } : undefined;
  let report;
  try {
    report = await checkUrl(`${BASE}?view=${view}`, policy, { ready: "html[data-app-ready]", interactions });
  } catch (e) {
    failed = true;
    console.log(`${view}: check did not finish: ${e.message}`);
    continue;
  }
  for (const f of report.findings) all.push({ view, ...f });
  const states = report.runs[0]?.states.map((s) => s.name).join(", ");
  console.log(`${view}: ${report.findings.length} findings (${states})`);
}

const byRule = new Map();
for (const f of all) {
  const key = `${f.rule}  ${f.state ?? ""}  ${f.selector}`;
  const seen = byRule.get(key) ?? { f, views: new Set(), n: 0 };
  seen.views.add(f.view);
  seen.n++;
  byRule.set(key, seen);
}
for (const [key, { f, views: v, n }] of [...byRule].sort((a, b) => a[0].localeCompare(b[0]))) {
  console.log(`\n${key}  (${n}x: ${[...v].join(", ")})\n  ${f.message}\n  Fix: ${f.fix}`);
}
if (jsonPath) await writeFile(jsonPath, JSON.stringify(all, null, 2));
console.log(`\n${all.length ? "Failed" : "Passed"}: ${all.length} findings, ${byRule.size} distinct.`);
process.exitCode = failed ? 2 : all.length ? 1 : 0;
