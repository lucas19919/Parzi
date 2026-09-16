// Picking the three models of a project roster (hub PLAN.md §1.2). Pure, so
// the "sensible defaults" of the New project wizard are testable: the catalog
// is the only input and the same catalog always gives the same three.

import type { ModelRow, Roster } from "../api";

/** One model a project may bind to a role, as `provider/model`. */
export interface Candidate {
  spec: string;
  label: string;
  provider: string;
  inPrice: number;
  outPrice: number;
}

/** Every tool-capable, non-legacy model of a provider we can actually reach. */
export function candidates(rows: ModelRow[]): Candidate[] {
  const out: Candidate[] = [];
  for (const r of rows) {
    if (r.auth !== "ok") continue;
    for (const m of r.models) {
      if (m.legacy || !m.tools) continue;
      out.push({
        spec: `${r.provider}/${m.id}`,
        label: m.name || m.id,
        provider: r.provider,
        inPrice: m.price_in,
        outPrice: m.price_out,
      });
    }
  }
  return out;
}

/** Coder families worth preferring when several models cost the same. */
const CODERS = ["codex", "coder", "muse", "sonnet", "fable", "devstral"];

const cost = (x: Candidate) => x.inPrice + x.outPrice;

/**
 * Sensible three: the header answers from state all day, so it takes the
 * cheapest model; the orchestrator writes the plan, so it takes the most
 * expensive (the only signal a catalog gives for "strongest"); the coder
 * takes the dearest model of a coding family, else the orchestrator's.
 */
export function defaultRoster(rows: ModelRow[]): Roster {
  const c = candidates(rows);
  if (!c.length) return { header: "", orchestrator: "", coder: "" };
  const cheapest = [...c].sort((a, b) => cost(a) - cost(b))[0];
  const strongest = [...c].sort((a, b) => cost(b) - cost(a))[0];
  const coder =
    [...c]
      .filter((x) => CODERS.some((k) => x.spec.toLowerCase().includes(k)))
      .sort((a, b) => cost(b) - cost(a))[0] ?? strongest;
  return { header: cheapest.spec, orchestrator: strongest.spec, coder: coder.spec };
}
