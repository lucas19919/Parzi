// Runs lintapple (ui-policy.json) over every main view of the app, served
// by `npm run dev` with the mock backend (mock.html). Exit 1 on findings.
//   npm run dev            (in another terminal)
//   npm run lint:ui        [--json report.json] [view ...]
import { readFile, writeFile } from "node:fs/promises";
import { checkUrl } from "lintapple";

const BASE = process.env.PARZI_UI_URL ?? "http://localhost:1420/mock.html";
const VIEWS = ["home", "session", "request", "brain", "history", "settings", "setup", "page"];
// Views that are another view in other settings.
const QUERY = { request: "view=session&request=file" };

// More states per view, each checked again after its action (run in order
// on one page). Selectors are CSS and must match exactly one element.
const click = (name, selector, ready) => ({ name, action: "click", selector, ready: { selector: ready } });
const nth = (list, i) => `${list} > :nth-child(${i})`;
const STATES = {
  home: [
    click("project menu", ".ob .proj", ".menu-pop"),
    // Pressed in the composer: the audit may already have scrolled the menu shut.
    { name: "project menu closed", action: "press", selector: ".ob textarea", key: "Escape", ready: { selector: ".menu-pop", state: "hidden" } },
    click("web", ".under .seg > button:last-child", ".box.web"),
    { name: "web suggestions", action: "fill", selector: ".ob textarea", value: "parzi docs", ready: { selector: ".web-suggest" } },
  ],
  session: [
    // The last turn's charts (```viz blocks): the line chart's readout.
    { name: "viz readout", action: "hover", selector: '.viz[data-viz="line"] svg.plot', ready: { selector: ".viz-tip:not([hidden])" } },
    { name: "switcher", action: "press", selector: ".ob textarea", key: "Control+p", ready: { selector: '.box[role="dialog"]' } },
    { name: "switcher closed", action: "press", selector: '.box[role="dialog"] input', key: "Escape", ready: { selector: '.box[role="dialog"]', state: "hidden" } },
    click("model picker", ".under .model", ".picker"),
    click("permission menu", '.under button[aria-haspopup="menu"]', ".menu-pop.perms"),
    click("session actions", '.session .menu button[aria-haspopup="menu"]', '.dropdown[role="menu"]'),
    // The plan checklist, opened from the session column: then its done
    // items, then an item quoted into the composer.
    click("plan checklist", ".info .plan-row", ".thread-col .plan ul"),
    click("plan done items", ".plan .item.fold", ".plan li:last-child > .item.closed"),
    click("plan quote chip", ".plan ul > li:nth-child(2) > .item", ".ob .chip.quote"),
    // A session that is working holds what you send until it is done.
    click("running session", ".tabs > .tab:nth-child(3) .pick", ".ob .go.stop"),
    { name: "typed ahead", action: "fill", selector: ".ob textarea", value: "Then add a test for it", ready: { selector: ".ob .go.stop" } },
    { name: "held", action: "press", selector: ".ob textarea", key: "Enter", ready: { selector: ".ob .held" } },
  ],
  brain: [click("pinned note", ".note:has(.pin)", ".titles h2"), click("conflict copy", ".note.copy", ".conflict")],
  history: [2, 3, 4].map((i) => click(`library ${i}`, nth(".seg", i), `${nth(".seg", i)}[aria-pressed="true"]`)),
  settings: [2, 3, 4, 5].map((i) => click(`section ${i}`, nth(".sections", i), `${nth(".sections", i)}[aria-selected="true"]`)),
  setup: [
    ...[2, 3].map((i) => click(`step ${i}`, nth("header .steps", i), `${nth("header .steps", i)}[aria-current="step"]`)),
    click("tool notes", ".body .list > .item:first-child .more", ".list.picks"),
    click("step 4", nth("header .steps", 4), `${nth("header .steps", 4)}[aria-current="step"]`),
    click("server form", ".disclose", ".remote-form"),
  ],
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
    report = await checkUrl(`${BASE}?${QUERY[view] ?? `view=${view}`}`, policy, { ready: "html[data-app-ready]", interactions });
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
