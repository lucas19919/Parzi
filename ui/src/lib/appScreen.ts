// What a person can press or fill in Parzi's window, read from the page as
// it is, so an agent can use any control, registered as an action or not.
// Each control gets a ref for app_click and app_type until the next look.

export interface Control {
  ref: number;
  role: string;
  label: string;
  where: string;
  state: string;
}

const INTERACTIVE = 'button, a[href], input, textarea, select, [role="button"], [role="tab"], [role="switch"], [role="checkbox"], [role="radio"], [role="option"], [role="menuitem"], summary';
const CONTAINERS = 'section, aside, nav, header, figure, dialog, [role="dialog"], [role="group"], [role="region"], [role="tablist"], [role="listbox"], [role="menu"], [aria-label]';
const HEADINGS = "h1, h2, h3, .t-page, .t-section, .row-title, legend";
/** Words on a control that mean it deletes, removes, resets or signs out. */
const DANGER = /\b(delete|remove|reset|sign out|log out|uninstall|discard|forget|clear all|erase)\b/i;
const CONFIRM_FOR = 15_000;

let refs = new Map<number, Element>();
const confirmedAt = new WeakMap<Element, number>();

const squash = (s: string | null | undefined) => (s ?? "").replace(/\s+/g, " ").trim();

function visible(el: Element): boolean {
  const r = el.getBoundingClientRect();
  // Below the fold still counts: a page scrolls, and a click reaches it.
  if (r.width < 2 || r.height < 2) return false;
  const cs = getComputedStyle(el);
  return cs.visibility !== "hidden" && cs.display !== "none" && Number(cs.opacity) > 0.05;
}

function roleOf(el: Element): string {
  const role = el.getAttribute("role");
  if (role) return role;
  const tag = el.tagName.toLowerCase();
  if (tag === "input") {
    const t = (el as HTMLInputElement).type;
    return t === "checkbox" ? "checkbox" : t === "radio" ? "radio" : t === "range" ? "slider" : "field";
  }
  if (tag === "textarea") return "field";
  if (tag === "select") return "choice";
  if (tag === "a") return "link";
  return "button";
}

function labelOf(el: Element): string {
  const own = squash(el.getAttribute("aria-label")) || squash(el.getAttribute("title"));
  if (own) return own;
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    const named = el.id ? document.querySelector(`label[for="${CSS.escape(el.id)}"]`) : null;
    return squash(named?.textContent) || squash(el.placeholder) || squash(el.name);
  }
  return squash((el as HTMLElement).innerText ?? el.textContent).slice(0, 80);
}

function stateOf(el: Element): string {
  const out: string[] = [];
  const pressed = el.getAttribute("aria-pressed") ?? el.getAttribute("aria-checked") ?? el.getAttribute("aria-selected");
  if (pressed === "true") out.push("on");
  else if (pressed === "false" && el.getAttribute("role") !== "tab") out.push("off");
  if (el.getAttribute("aria-expanded") === "true") out.push("open");
  if ((el as HTMLButtonElement).disabled || el.getAttribute("aria-disabled") === "true") out.push("disabled");
  if (el instanceof HTMLInputElement && (el.type === "checkbox" || el.type === "radio")) out.push(el.checked ? "on" : "off");
  else if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement) {
    const v = squash(el.value);
    if (v && el.type !== "password") out.push(`value "${v.slice(0, 60)}"`);
  }
  return out.join(", ");
}

/** The card, section or page a control sits in, by its heading or name. */
function whereOf(el: Element): string {
  const names: string[] = [];
  let node: Element | null = el.parentElement;
  for (let depth = 0; node && node !== document.body && depth < 12 && names.length < 2; depth++, node = node.parentElement) {
    if (!node.matches(CONTAINERS)) continue;
    const heading = node.querySelector(HEADINGS);
    const name = squash(node.getAttribute("aria-label")) || (heading && !heading.contains(el) ? squash(heading.textContent) : "");
    if (name && !names.includes(name) && name.length < 60) names.unshift(name);
  }
  return names.join(" › ");
}

/** Every visible control, numbered, as an agent reads it. */
export function screen(query = "", where = ""): string {
  refs = new Map();
  const q = query.trim().toLowerCase();
  const rows: string[] = [];
  let n = 0;
  for (const el of document.querySelectorAll(INTERACTIVE)) {
    if (!visible(el) || el.closest("[inert], [aria-hidden='true']")) continue;
    const label = labelOf(el);
    const role = roleOf(el);
    if (!label && role === "button") continue;
    const place = whereOf(el);
    if (q && !`${label} ${place}`.toLowerCase().includes(q)) continue;
    n += 1;
    refs.set(n, el);
    const state = stateOf(el);
    rows.push(`[${n}] ${role} "${label || "(no label)"}"${state ? ` (${state})` : ""}${place ? ` in ${place}` : ""}`);
    if (n >= 300) break;
  }
  const head = where ? `${where}\n` : "";
  if (!rows.length) return `${head}${q ? `Nothing on screen matches "${query}".` : "Nothing to press on screen."}`;
  return `${head}${rows.join("\n")}`;
}

function controlAt(ref: number): Element {
  const el = refs.get(Number(ref));
  if (!el || !el.isConnected) throw new Error(`No control [${ref}] on screen now; look again with app_screen.`);
  return el;
}

/** Clicks a control by ref; one that deletes, removes, resets or signs out
 *  asks for the person's yes first, once per control for a little while. */
export function click(ref: number, confirmed = false): { text: string } | { needs_yes: string } {
  const el = controlAt(ref);
  const label = labelOf(el) || roleOf(el);
  if ((el as HTMLButtonElement).disabled) throw new Error(`"${label}" is disabled right now.`);
  const risky = DANGER.test(label) || el.matches(".danger, .btn.danger");
  const recent = (confirmedAt.get(el) ?? 0) > Date.now() - CONFIRM_FOR;
  if (risky && !confirmed && !recent) return { needs_yes: `Click "${label}"` };
  if (risky) confirmedAt.set(el, Date.now());
  (el as HTMLElement).focus?.();
  (el as HTMLElement).click();
  return { text: `Clicked "${label}". Look again with app_screen if it opened something.` };
}

/** Fills a field by ref, the way typing would, and presses Enter if asked. */
export function type(ref: number, text: string, enter = false): { text: string } {
  const el = controlAt(ref);
  if (!(el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement || el instanceof HTMLSelectElement)) {
    throw new Error(`[${ref}] is not a field; click it instead.`);
  }
  el.focus();
  if (el instanceof HTMLSelectElement) {
    const opt = [...el.options].find((o) => o.value === text || squash(o.textContent).toLowerCase() === text.toLowerCase());
    if (!opt) throw new Error(`"${text}" is not one of ${[...el.options].map((o) => squash(o.textContent)).join(", ")}.`);
    el.value = opt.value;
  } else {
    const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, "value")?.set?.call(el, text);
  }
  el.dispatchEvent(new Event("input", { bubbles: true }));
  el.dispatchEvent(new Event("change", { bubbles: true }));
  if (enter) el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true, cancelable: true }));
  return { text: `Typed into "${labelOf(el) || roleOf(el)}"${enter ? " and pressed Enter" : ""}.` };
}
