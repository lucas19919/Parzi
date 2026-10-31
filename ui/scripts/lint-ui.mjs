import { readFile, writeFile } from "node:fs/promises";
import { checkUrl } from "lintapple";

const BASE = process.env.PARZI_UI_URL ?? "http://localhost:1420/mock.html";
const VIEWS = ["home", "session", "request", "question", "approval", "manager", "settings", "setup", "page"];
const QUERY = { request: "view=session&request=file", question: "view=session&question=1", approval: "view=session&approval=1" };

const click = (name, selector, ready) => ({ name, action: "click", selector, ready: { selector: ready } });
const nth = (list, i) => `${list} > :nth-child(${i})`;
const STATES = {
  home: [
    click("project menu", ".ob .where-name", ".menu-pop"),
    { name: "project menu closed", action: "press", selector: ".ob textarea", key: "Escape", ready: { selector: ".menu-pop", state: "hidden" } },
    click("web", '.ob .icon[aria-label="Browser"]', ".box.web"),
    { name: "web suggestions", action: "fill", selector: ".ob textarea", value: "parzi docs", ready: { selector: ".web-suggest" } },
  ],
  session: [
    { name: "viz readout", action: "hover", selector: '.viz[data-viz="line"] svg.plot', ready: { selector: ".viz-tip:not([hidden])" } },
    { name: "switcher", action: "press", selector: ".ob textarea", key: "Control+p", ready: { selector: '.box[role="dialog"]' } },
    { name: "switcher closed", action: "press", selector: '.box[role="dialog"] input', key: "Escape", ready: { selector: '.box[role="dialog"]', state: "hidden" } },
    click("model picker", ".ob .model", ".picker"),
    click("permission menu", ".ob .perm-btn", ".menu-pop"),
    click("plan checklist", ".info .plan-row", ".thread-col .plan ul"),
    click("plan done items", ".plan .item.fold", ".plan li:last-child > .item.closed"),
    click("plan quote chip", ".plan ul > li:nth-child(2) > .item", ".ob .chip.quote"),
    click("running session", ".tabs > .tab:nth-child(3) .pick", ".ob .go.stop"),
    { name: "typed ahead", action: "fill", selector: ".ob textarea", value: "Then add a test for it", ready: { selector: ".ob .go.ready" } },
    { name: "held", action: "press", selector: ".ob textarea", key: "Enter", ready: { selector: ".ob .queue-item" } },
  ],
  manager: [
    { name: "manager search", action: "fill", selector: ".page input", value: "sidebar", ready: { selector: ".page .sess .row-main" } },
    click("manager open", ".page .sess .row-main", ".stagerow .session"),
  ],
  settings: [
    ...[2, 3].map((i) => click(`section ${i}`, nth(".sections", i), `${nth(".sections", i)}[aria-selected="true"]`)),
    click("connector tools", ".rows .conn .row-main", ".rows .details .row:first-child"),
    click("section 4", nth(".sections", 4), `${nth(".sections", 4)}[aria-selected="true"]`),
  ],
  setup: [
    ...[2, 3].map((i) => click(`step ${i}`, nth("header .page-tabs", i), `${nth("header .page-tabs", i)}[aria-selected="true"]`)),
    click("tool notes", ".body .list > .item:first-child .more", ".list.picks"),
    click("step 4", nth("header .page-tabs", 4), `${nth("header .page-tabs", 4)}[aria-selected="true"]`),
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
