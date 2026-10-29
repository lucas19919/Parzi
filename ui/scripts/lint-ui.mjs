import { readFile, writeFile } from "node:fs/promises";
import { checkUrl } from "lintapple";

const BASE = process.env.PARZI_UI_URL ?? "http://localhost:1420/mock.html";
const VIEWS = ["home", "session", "request", "question", "approval", "history", "settings", "setup", "page"];
const QUERY = { request: "view=session&request=file", question: "view=session&question=1", approval: "view=session&approval=1" };

const click = (name, selector, ready) => ({ name, action: "click", selector, ready: { selector: ready } });
const nth = (list, i) => `${list} > :nth-child(${i})`;
const STATES = {
  home: [
    click("project menu", ".ob .where-name", ".menu-pop"),
    { name: "project menu closed", action: "press", selector: ".ob textarea", key: "Escape", ready: { selector: ".menu-pop", state: "hidden" } },
    click("web", ".ob .browser", ".box.web"),
    { name: "web suggestions", action: "fill", selector: ".ob textarea", value: "parzi docs", ready: { selector: ".web-suggest" } },
  ],
  session: [
    { name: "viz readout", action: "hover", selector: '.viz[data-viz="line"] svg.plot', ready: { selector: ".viz-tip:not([hidden])" } },
    { name: "switcher", action: "press", selector: ".ob textarea", key: "Control+p", ready: { selector: '.box[role="dialog"]' } },
    { name: "switcher closed", action: "press", selector: '.box[role="dialog"] input', key: "Escape", ready: { selector: '.box[role="dialog"]', state: "hidden" } },
    click("model picker", ".ob .model", ".picker"),
    click("permission menu", ".ob .perm-btn", ".menu-pop.perms"),
    click("plan checklist", ".info .plan-row", ".thread-col .plan ul"),
    click("plan done items", ".plan .item.fold", ".plan li:last-child > .item.closed"),
    click("plan quote chip", ".plan ul > li:nth-child(2) > .item", ".ob .chip.quote"),
    click("running session", ".tabs > .tab:nth-child(3) .pick", ".ob .go.stop"),
    { name: "typed ahead", action: "fill", selector: ".ob textarea", value: "Then add a test for it", ready: { selector: ".ob .go.stop" } },
    { name: "held", action: "press", selector: ".ob textarea", key: "Enter", ready: { selector: ".ob .held" } },
  ],
  history: [2, 3, 4].map((i) => click(`library ${i}`, nth(".seg", i), `${nth(".seg", i)}[aria-pressed="true"]`)),
  settings: [
    ...[2, 3].map((i) => click(`section ${i}`, nth(".sections", i), `${nth(".sections", i)}[aria-selected="true"]`)),
    click("connector directory", ".add-conn", '.card[role="dialog"] .grid'),
    click("connector custom", ".card footer .link", ".card textarea.mono"),
    { name: "connector address", action: "fill", selector: ".card textarea.mono", value: "https://mcp.example.com/mcp", ready: { selector: '.card input[type="password"]' } },
    click("connector back", ".card header .icon:first-child", '.card[role="dialog"] .grid'),
    { name: "connector closed", action: "click", selector: ".card header .icon:last-child", ready: { selector: '.card[role="dialog"]', state: "hidden" } },
    click("connector connected", ".conn .sbtn", ".conn .sbtn[aria-expanded]"),
    click("connector tools", ".conn .sbtn[aria-expanded]", ".tools .tool:first-child"),
    ...[4, 5].map((i) => click(`section ${i}`, nth(".sections", i), `${nth(".sections", i)}[aria-selected="true"]`)),
  ],
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
