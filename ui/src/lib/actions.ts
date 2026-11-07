// Everything Parzi can do, by name. Surfaces register what they can do
// (open a tab, pick a model, set the theme) and agents run it with app_do,
// so a person's click and an agent's request take the same path. What is
// not registered here an agent still reaches through appScreen.ts.

export interface ActionArg {
  name: string;
  type: "string" | "number" | "boolean";
  about?: string;
  /** The only values it takes. */
  choices?: string[] | (() => string[]);
  required?: boolean;
}

export interface AppAction {
  /** group.verb, such as tabs.new or composer.model. */
  id: string;
  title: string;
  args?: ActionArg[];
  /** Deletes, removes, resets or signs out: the person says yes first. */
  danger?: boolean;
  /** Whether it can run right now (a session to stop, a page to close). */
  when?: () => boolean;
  run: (args: Record<string, unknown>) => string | void | Promise<string | void>;
}

export type ActionResult = { text: string } | { needs_yes: string };

const registry = new Map<string, AppAction>();

/** Registers actions; the returned function takes them out again. */
export function defineActions(list: AppAction[]): () => void {
  for (const a of list) registry.set(a.id, a);
  return () => {
    for (const a of list) if (registry.get(a.id) === a) registry.delete(a.id);
  };
}

const choicesOf = (a: ActionArg): string[] => (typeof a.choices === "function" ? a.choices() : (a.choices ?? []));

function ready(a: AppAction): boolean {
  try {
    return a.when ? a.when() : true;
  } catch {
    return false;
  }
}

export function availableActions(query = ""): AppAction[] {
  const q = query.trim().toLowerCase();
  return [...registry.values()]
    .filter(ready)
    .filter((a) => !q || a.id.toLowerCase().includes(q) || a.title.toLowerCase().includes(q))
    .sort((a, b) => a.id.localeCompare(b.id));
}

function argText(a: ActionArg): string {
  const choices = choicesOf(a);
  const kind = choices.length ? choices.slice(0, 12).join("|") + (choices.length > 12 ? "|…" : "") : a.type;
  return `${a.name}${a.required ? "" : "?"}: ${kind}${a.about ? ` (${a.about})` : ""}`;
}

/** The actions as an agent reads them, one per line. */
export function describeActions(query = ""): string {
  const list = availableActions(query);
  if (!list.length) return query ? `No action matches "${query}". Leave out query to see them all, or use app_screen.` : "No actions right now.";
  return list.map((a) => `${a.id}: ${a.title}${a.args?.length ? ` { ${a.args.map(argText).join(", ")} }` : ""}${a.danger ? " (asks the person)" : ""}`).join("\n");
}

function coerce(a: ActionArg, raw: unknown): unknown {
  if (raw === undefined || raw === null || raw === "") return undefined;
  if (a.type === "number") {
    const n = typeof raw === "number" ? raw : Number(String(raw).trim());
    if (!Number.isFinite(n)) throw new Error(`${a.name} is a number`);
    return n;
  }
  if (a.type === "boolean") {
    if (typeof raw === "boolean") return raw;
    const t = String(raw).trim().toLowerCase();
    if (["true", "yes", "on", "1"].includes(t)) return true;
    if (["false", "no", "off", "0"].includes(t)) return false;
    throw new Error(`${a.name} is true or false`);
  }
  const s = String(raw).trim();
  const choices = choicesOf(a);
  if (choices.length) {
    const hit = choices.find((c) => c.toLowerCase() === s.toLowerCase());
    if (!hit) throw new Error(`${a.name} is one of ${choices.join(", ")}, not "${s}"`);
    return hit;
  }
  return s;
}

/** Runs an action by id with the agent's arguments, checked and coerced. */
export async function runAction(id: string, args: Record<string, unknown> = {}, confirmed = false): Promise<ActionResult> {
  const a = registry.get(id.trim());
  if (!a) {
    const close = availableActions(id.split(".")[0]).slice(0, 6).map((x) => x.id);
    throw new Error(`No action "${id}".${close.length ? ` Try ${close.join(", ")}.` : " See app_actions."}`);
  }
  if (!ready(a)) throw new Error(`${id} can't run right now.`);
  const clean: Record<string, unknown> = {};
  for (const spec of a.args ?? []) {
    const v = coerce(spec, args[spec.name]);
    if (v === undefined && spec.required) throw new Error(`${id} needs ${argText(spec)}`);
    if (v !== undefined) clean[spec.name] = v;
  }
  if (a.danger && !confirmed) return { needs_yes: a.title };
  const said = await a.run(clean);
  return { text: said || `Done: ${a.title}.` };
}
