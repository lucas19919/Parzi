// Checks corners on the rendered mock UI, which the static token lint cannot:
// every control and picture of 16px or more has the one box corner
// (--radius), and no element anywhere has a corner off the scale.
//
//   npm run dev            (in another terminal)
//   npm run lint:radii     (PARZI_UI_URL overrides the mock address)
//
// Uses Playwright from the machine (`npm i -g playwright` or PLAYWRIGHT=path).
const BASE = process.env.PARZI_UI_URL ?? "http://localhost:1420/mock.html";
const VIEWS = {
  home: "view=home",
  session: "view=session",
  question: "view=session&question=1",
  approval: "view=session&approval=1",
  manager: "view=manager",
  settings: "view=settings",
  setup: "view=setup",
  page: "view=page",
};
// Popovers close with Escape before the next one opens; tabs stay.
const POPOVERS = new Set(["home", "session"]);
const CLICKS = {
  home: [".ob .where-name", '.ob .icon[aria-label="Browser"]'],
  session: [".ob .model", ".ob .perm-btn", ".info .plan-row"],
  settings: [2, 3, 4].map((i) => `.sections > :nth-child(${i})`),
  setup: [2, 3, 4].map((i) => `header .page-tabs > :nth-child(${i})`),
};

const { chromium } = await import(process.env.PLAYWRIGHT ?? "playwright").catch(async () => {
  const { execSync } = await import("node:child_process");
  const root = execSync("npm root -g").toString().trim();
  return import(`${root}/playwright/index.mjs`);
});

function check() {
  const probe = document.createElement("i");
  document.body.append(probe);
  probe.style.borderRadius = "var(--radius)";
  const box = parseFloat(getComputedStyle(probe).borderTopLeftRadius);
  probe.style.borderRadius = "var(--radius-sm)";
  const mark = parseFloat(getComputedStyle(probe).borderTopLeftRadius);
  probe.remove();
  const scale = new Set([0, mark, box]);
  const found = [];
  const name = (el) => {
    const cls = [...el.classList].filter((c) => !c.startsWith("s-") && !c.startsWith("svelte-"));
    const parent = el.parentElement;
    const pc = parent ? [...parent.classList].filter((c) => !c.startsWith("s-") && !c.startsWith("svelte-")) : [];
    return `${parent ? parent.tagName.toLowerCase() + (pc.length ? "." + pc.join(".") : "") + " > " : ""}${el.tagName.toLowerCase()}${cls.length ? "." + cls.join(".") : ""}`;
  };
  for (const el of document.querySelectorAll("body *")) {
    if (el.closest(".preview, .art iframe, .msg, .viz, .switch, .slider")) continue;
    const cs = getComputedStyle(el);
    if (cs.display === "none" || cs.visibility === "hidden") continue;
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) continue;
    const corners = [cs.borderTopLeftRadius, cs.borderTopRightRadius, cs.borderBottomRightRadius, cs.borderBottomLeftRadius];
    if (corners.some((c) => c.endsWith("%"))) continue;
    const px = corners.map(parseFloat);
    const odd = px.filter((c) => !scale.has(c) && c < 999);
    if (odd.length) {
      found.push(`${name(el)} ${Math.round(r.width)}x${Math.round(r.height)}: corner ${odd[0]}px is off the scale`);
      continue;
    }
    const control = el.matches("button, input:not([type=range]), select, textarea, [role=button], [role=tab], [role=option], img");
    if (control && r.width >= 16 && r.height >= 16 && px.some((c) => c === mark)) {
      found.push(`${name(el)} ${Math.round(r.width)}x${Math.round(r.height)}: a control or picture has the mark corner, use --radius`);
    }
  }
  return found;
}

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
const problems = new Set();
for (const [view, query] of Object.entries(VIEWS)) {
  await page.goto(`${BASE}?${query}`);
  await page.waitForSelector("html[data-app-ready]", { timeout: 15000 });
  await page.waitForTimeout(600);
  for (const p of await page.evaluate(check)) problems.add(`${view}: ${p}`);
  for (const sel of CLICKS[view] ?? []) {
    await page.click(sel, { timeout: 3000 }).catch(() => problems.add(`${view}: could not open ${sel}`));
    await page.waitForTimeout(300);
    for (const p of await page.evaluate(check)) problems.add(`${view}: ${p}`);
    if (POPOVERS.has(view)) await page.keyboard.press("Escape");
  }
}
await browser.close();
if (problems.size) {
  console.error([...problems].join("\n"));
  console.error(`\n${problems.size} radius problems`);
  process.exit(1);
}
console.log("radii ok");
