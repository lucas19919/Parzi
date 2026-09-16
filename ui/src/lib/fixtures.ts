/**
 * Screenshot fixtures for the workspace wizard's last step. The verifier
 * launches with `PARZI_UI_STATE=new-workspace:init` (every repo ready) or
 * `new-workspace:init-failed` (one clone that did not work), and the wizard
 * draws that clone map without a network, a daemon or a single write.
 *
 * The project deck's own fixtures live beside it, in `deck/fixtures.ts`.
 */

import type { RepoRef } from "./api";

/** One row of the Initialise step's clone map, as `parzi://repo-clone` sends it. */
export interface CloneProgress {
  phase: "cloning" | "done" | "error";
  line: string;
}

/** What the Initialise step should show instead of an empty map. */
export interface InitFixture {
  /** Workspace name, so the step's copy reads like a real one. */
  name: string;
  /** The repos this run picked — what the step's Retry would run again. */
  refs: RepoRef[];
  progress: Record<string, CloneProgress>;
  /** Every repo is ready: the primary button moves on instead of waiting. */
  done: boolean;
}

const NAME = "acme";

const ref = (name: string): RepoRef => ({
  name,
  remote: `git@github.com:${NAME}/${name}.git`,
  default_branch: "main",
  local_path: null,
});

const REFS: RepoRef[] = [ref("shop-api"), ref("shop-web"), ref("shop-crm")];

const READY: Record<string, CloneProgress> = {
  "shop-api": { phase: "done", line: "~/Parzi/acme/shop-api" },
  "shop-web": { phase: "done", line: "~/Parzi/acme/shop-web" },
  "shop-crm": { phase: "done", line: "~/Parzi/acme/shop-crm" },
};

const ONE_FAILED: Record<string, CloneProgress> = {
  "shop-api": { phase: "done", line: "~/Parzi/acme/shop-api" },
  "shop-web": { phase: "error", line: "remote: Repository not found (exit 128)" },
  "shop-crm": { phase: "done", line: "~/Parzi/acme/shop-crm" },
};

/**
 * The fixture for a wizard step name, or null when this is a real run. The
 * step name is everything after the colon of PARZI_UI_STATE, so only the
 * verifier ever reaches these.
 */
export function initFixture(step: string): InitFixture | null {
  const want = step.trim();
  if (want === "init") return { name: NAME, refs: REFS, progress: READY, done: true };
  if (want === "init-failed") return { name: NAME, refs: REFS, progress: ONE_FAILED, done: false };
  return null;
}
