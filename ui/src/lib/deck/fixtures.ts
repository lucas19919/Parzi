/**
 * Screenshot fixtures for the project deck. The verifier sets
 * `PARZI_UI_STATE=deck:plan` (shell lane) and the deck renders from these
 * instead of the commands — no model, no files, no daemon.
 */
import { invoke } from "@tauri-apps/api/core";
import type { AuditResult, Draft, JournalLine, Plan, Project } from "../api";

export type DeckFixtureState = "deck:project" | "deck:plan" | "deck:activity" | "deck:audit-summary" | "deck:settings" | "deck:stage";

export interface DeckFixture {
  state: DeckFixtureState;
  project: Project;
  plan: Plan;
  journal: JournalLine[];
  drafts: Draft[];
  /** STATUS.md as `project_status` renders it (Activity tab summary). */
  status: string;
  /** Set for `deck:audit-summary`: the summary card waits for Approve. */
  audit: AuditResult | null;
}

const STATES: DeckFixtureState[] = ["deck:project", "deck:plan", "deck:activity", "deck:audit-summary", "deck:settings", "deck:stage"];

/**
 * The requested screenshot state, from (in order) an explicit prop, the
 * window global the shell sets from PARZI_UI_STATE, the Vite env, or a
 * `?ui_state=` query. Null = the real commands.
 */
export function deckFixtureState(explicit?: string | null): DeckFixtureState | null {
  const w = typeof window !== "undefined" ? (window as unknown as Record<string, string>) : {};
  const q = typeof location !== "undefined" ? new URLSearchParams(location.search).get("ui_state") : null;
  const raw =
    explicit ||
    w.__PARZI_UI_STATE ||
    (import.meta.env?.VITE_PARZI_UI_STATE as string | undefined) ||
    q ||
    "";
  return STATES.find((s) => s === raw) ?? null;
}

/** Same, plus the host's `ui_state` command (PARZI_UI_STATE at launch). */
export async function deckFixtureStateAsync(explicit?: string | null): Promise<DeckFixtureState | null> {
  const local = deckFixtureState(explicit);
  if (local) return local;
  try {
    return deckFixtureState(await invoke<string>("ui_state"));
  } catch {
    // No host (browser) or no such command yet: the deck uses real data.
    return null;
  }
}

const PROJECT: Project = {
  slug: "checkout-flow",
  title: "Checkout flow",
  workspace: "acme",
  repos: ["shop-api", "shop-web"],
  roster: {
    header: "google/gemini-2.5-flash",
    orchestrator: "anthropic/claude-fable-5-1",
    coder: "opencode/muse-spark-1.3",
  },
  budget_usd: 40,
  status: "running",
  critical: ["shop-api/src/payments/**"],
  why: "Card checkout still bounces people to a hosted page. We want the payment intent to live in our API so the web app can finish the sale in one flow.",
  what: [
    { text: "A checkout session can be created and paid in one page", done: true },
    { text: "Payment intents are idempotent under retry", done: false },
    { text: "No card data ever reaches shop-web", done: false },
  ],
  constraints: ["no schema migrations this sprint", "keep the hosted page working until sprint 2"],
};

const PLAN: Plan = {
  sprints: [
    {
      title: "Sprint 1 — API surface",
      target: "1 day",
      lanes: [
        {
          name: "api",
          tasks: [
            {
              id: "TSK-7",
              title: "Checkout session endpoint",
              repo: "shop-api",
              scope: ["src/checkout/**", "src/routes.rs"],
              after: [],
              critical: false,
              done: true,
              acceptance: ["POST /checkout returns a session id", "409 on a paid session"],
            },
            {
              id: "TSK-8",
              title: "Payment intent adapter",
              repo: "shop-api",
              scope: ["src/payments/**", "src/payments/stripe.rs", "src/config.rs"],
              after: ["TSK-7"],
              critical: true,
              done: false,
              acceptance: ["intent is idempotent per session"],
            },
          ],
        },
        {
          name: "web",
          tasks: [
            {
              id: "TSK-9",
              title: "Checkout page skeleton",
              repo: "shop-web",
              scope: ["src/routes/checkout/**"],
              after: ["TSK-7"],
              critical: false,
              done: false,
              acceptance: ["page renders the session summary"],
            },
          ],
        },
      ],
    },
    {
      title: "Sprint 2 — wire and test",
      target: "2 days",
      lanes: [
        {
          name: "api",
          tasks: [
            {
              id: "TSK-11",
              title: "Webhook reconciliation",
              repo: "shop-api",
              scope: ["src/payments/webhook.rs"],
              after: ["TSK-8"],
              critical: false,
              done: false,
              acceptance: ["a replayed webhook changes nothing"],
            },
          ],
        },
        {
          name: "web",
          tasks: [
            {
              id: "TSK-12",
              title: "Pay button and result states",
              repo: "shop-web",
              scope: ["src/routes/checkout/pay.svelte"],
              after: ["TSK-9", "TSK-8"],
              critical: false,
              done: false,
              acceptance: ["failure shows the decline reason"],
            },
          ],
        },
      ],
    },
  ],
};

/**
 * A third lane, only for `deck:plan`: three lanes are wider than a 1100px
 * window, which is what the sticky sprint column and the scroll affordance
 * exist for. The other states keep the two-lane plan, so STATUS.md and the
 * journal below still count the same five tasks.
 */
const PLAN_WIDE: Plan = {
  sprints: PLAN.sprints.map((s, i) => ({
    ...s,
    lanes: [
      ...s.lanes,
      {
        name: "infra",
        tasks: i === 0
          ? [
              {
                id: "TSK-10",
                title: "Stripe test keys in the run env",
                repo: "shop-api",
                scope: ["deploy/env/**"],
                after: [],
                critical: false,
                done: false,
                acceptance: ["the sandbox key never reaches a build artefact"],
              },
            ]
          : [
              {
                id: "TSK-13",
                title: "Webhook endpoint behind the proxy",
                repo: "shop-api",
                scope: ["deploy/caddy/**"],
                after: ["TSK-11"],
                critical: false,
                done: false,
                acceptance: ["a signed webhook reaches the API in staging"],
              },
            ],
      },
    ],
  })),
};

const JOURNAL: JournalLine[] = [
  { at: "2026-09-16T09:02:11Z", who: "ada", kind: "approve", task: null, text: "plan approved, sprint 1 dispatched" },
  { at: "2026-09-16T09:02:12Z", who: "lane api", kind: "claim", task: "TSK-7", text: "claimed 2 paths in shop-api" },
  { at: "2026-09-16T09:06:40Z", who: "lane api", kind: "handoff", task: "TSK-7", text: "capsule written, 4 files touched, cargo test 31 passed" },
  { at: "2026-09-16T09:06:41Z", who: "lane api", kind: "release", task: "TSK-7", text: "released src/checkout/**, src/routes.rs" },
  { at: "2026-09-16T09:07:03Z", who: "lane api", kind: "claim", task: "TSK-8", text: "claimed 3 paths in shop-api" },
  { at: "2026-09-16T09:07:20Z", who: "lane web", kind: "claim", task: "TSK-9", text: "claimed 1 path in shop-web" },
  { at: "2026-09-16T09:08:02Z", who: "lane web", kind: "request", task: "TSK-9", text: "asks shop-api/src/routes.rs from lane api" },
  { at: "2026-09-16T09:08:31Z", who: "lane api", kind: "note", task: "TSK-8", text: "tool:edit turn:6 on src/payments/stripe.rs" },
  { at: "2026-09-16T09:09:10Z", who: "lane web", kind: "block", task: "TSK-9", text: "waiting on shop-api/src/routes.rs" },
];

/**
 * Thirty lines for `deck:activity`: enough that the journal is plainly the
 * body of the tab and the filter chips have something to filter. It ends
 * where the short one ends — TSK-7 done, TSK-8 editing on turn 6, TSK-9
 * blocked — so it still agrees with STATUS below.
 */
const JOURNAL_LONG: JournalLine[] = [
  { at: "2026-09-16T09:02:11Z", who: "ada", kind: "approve", task: null, text: "plan approved, sprint 1 dispatched" },
  { at: "2026-09-16T09:02:12Z", who: "lane api", kind: "claim", task: "TSK-7", text: "claimed 2 paths in shop-api" },
  { at: "2026-09-16T09:02:13Z", who: "lane api", kind: "note", task: "TSK-7", text: "tool:read turn:1 on src/routes.rs" },
  { at: "2026-09-16T09:02:58Z", who: "lane api", kind: "note", task: "TSK-7", text: "tool:edit turn:2 on src/checkout/session.rs" },
  { at: "2026-09-16T09:03:31Z", who: "lane web", kind: "note", task: null, text: "worktree ready at ~/Parzi/acme/shop-web" },
  { at: "2026-09-16T09:03:44Z", who: "lane api", kind: "note", task: "TSK-7", text: "tool:edit turn:3 on src/routes.rs" },
  { at: "2026-09-16T09:04:10Z", who: "lane api", kind: "note", task: "TSK-7", text: "tool:bash turn:4 cargo test -p shop-api" },
  { at: "2026-09-16T09:04:52Z", who: "ada", kind: "note", task: null, text: "reminder: no schema migrations this sprint" },
  { at: "2026-09-16T09:05:09Z", who: "lane api", kind: "note", task: "TSK-7", text: "tool:edit turn:5 on src/checkout/mod.rs" },
  { at: "2026-09-16T09:05:40Z", who: "lane api", kind: "note", task: "TSK-7", text: "tool:bash turn:6 cargo test -p shop-api" },
  { at: "2026-09-16T09:06:40Z", who: "lane api", kind: "handoff", task: "TSK-7", text: "capsule written, 4 files touched, cargo test 31 passed" },
  { at: "2026-09-16T09:06:41Z", who: "lane api", kind: "release", task: "TSK-7", text: "released src/checkout/**, src/routes.rs" },
  { at: "2026-09-16T09:06:55Z", who: "lane api", kind: "note", task: null, text: "knowledge: a checkout session id is a ULID, not a uuid" },
  { at: "2026-09-16T09:07:03Z", who: "lane api", kind: "claim", task: "TSK-8", text: "claimed 3 paths in shop-api" },
  { at: "2026-09-16T09:07:12Z", who: "lane api", kind: "note", task: "TSK-8", text: "tool:read turn:1 on src/payments/mod.rs" },
  { at: "2026-09-16T09:07:20Z", who: "lane web", kind: "claim", task: "TSK-9", text: "claimed 1 path in shop-web" },
  { at: "2026-09-16T09:07:31Z", who: "lane web", kind: "note", task: "TSK-9", text: "tool:read turn:1 on src/routes/checkout/+page.svelte" },
  { at: "2026-09-16T09:07:48Z", who: "lane api", kind: "note", task: "TSK-8", text: "tool:edit turn:2 on src/payments/stripe.rs" },
  { at: "2026-09-16T09:08:02Z", who: "lane web", kind: "request", task: "TSK-9", text: "asks shop-api/src/routes.rs from lane api" },
  { at: "2026-09-16T09:08:04Z", who: "ada", kind: "convene", task: null, text: "lane api and lane web on src/routes.rs" },
  { at: "2026-09-16T09:08:12Z", who: "lane api", kind: "deny", task: "TSK-9", text: "still editing routes.rs — TSK-9 runs after TSK-8" },
  { at: "2026-09-16T09:08:18Z", who: "lane api", kind: "note", task: "TSK-8", text: "tool:edit turn:3 on src/payments/mod.rs" },
  { at: "2026-09-16T09:08:26Z", who: "lane web", kind: "note", task: "TSK-9", text: "tool:read turn:2 on src/lib/checkout.ts" },
  { at: "2026-09-16T09:08:31Z", who: "lane api", kind: "note", task: "TSK-8", text: "tool:edit turn:4 on src/config.rs" },
  { at: "2026-09-16T09:08:44Z", who: "ada", kind: "plan_changed", task: null, text: "sprint 2 target moved to 2 days" },
  { at: "2026-09-16T09:08:50Z", who: "lane api", kind: "note", task: "TSK-8", text: "tool:bash turn:5 cargo check -p shop-api" },
  { at: "2026-09-16T09:09:02Z", who: "lane web", kind: "request", task: "TSK-9", text: "asks shop-api/src/routes.rs again" },
  { at: "2026-09-16T09:09:05Z", who: "lane api", kind: "grant", task: "TSK-9", text: "routes.rs goes to lane web after this turn" },
  { at: "2026-09-16T09:09:08Z", who: "lane api", kind: "note", task: "TSK-8", text: "tool:edit turn:6 on src/payments/stripe.rs" },
  { at: "2026-09-16T09:09:10Z", who: "lane web", kind: "block", task: "TSK-9", text: "waiting on shop-api/src/routes.rs" },
];

const DRAFTS: Draft[] = [
  {
    name: "1.md",
    title: "One API session, one page",
    content:
      "# One API session, one page\n\nThe API owns a checkout session; the web app renders it and posts a pay intent. Two lanes: api builds the session and the intent, web builds the page.\n\n- shop-api: session endpoint, payment intent adapter, webhook reconcile\n- shop-web: checkout page, pay button, result states\n\nRisk: payments code is the one place a mistake costs money — mark it critical.\n",
    path: "drafts/1.md",
  },
  {
    name: "2.md",
    title: "Keep the hosted page, wrap it",
    content:
      "# Keep the hosted page, wrap it\n\nCheaper route: keep the hosted page for one more sprint and only move session creation into shop-api. Less to build, less to prove, slower payoff.\n",
    path: "drafts/2.md",
  },
];

const AUDIT: AuditResult = {
  summary: [
    "Draft 1 holds up. Two lanes, two sprints, 5 tasks.",
    "",
    "- Sprint 1 (1 day): api builds TSK-7 session endpoint and TSK-8 intent",
    "  adapter; web builds TSK-9 page skeleton after TSK-7.",
    "- Sprint 2 (2 days): api reconciles webhooks (TSK-11), web wires the pay",
    "  button and its result states (TSK-12).",
    "- TSK-8 touches shop-api/src/payments/** which PROJECT.md marks critical:",
    "  a lease transfer there needs a person, not the coder.",
    "- TSK-9 will want src/routes.rs while api holds it; the plan orders it",
    "  after TSK-7 so the collision is one request, not a loop.",
    "- Budget 40 USD covers 5 coder tasks at the current per-task median.",
    "",
    "Unresolved: no test fixture exists for declined cards, and TSK-12 must show",
    "the decline reason — sprint 2 needs one.",
  ].join("\n"),
  plan: PLAN,
};

const STATUS = [
  "# checkout-flow — acme",
  "status: running · sprint 1/2 · 1/5 tasks done · budget 40 USD",
  "",
  "lane api   TSK-8  running   tool:edit turn 6   holds src/payments/**, src/payments/stripe.rs, src/config.rs",
  "lane web   TSK-9  blocked   waiting on shop-api/src/routes.rs (asked lane api 09:08)",
  "",
  "done: TSK-7 (capsule: 4 files, cargo test 31 passed)",
  "open: TSK-11, TSK-12 in sprint 2",
].join("\n");

export function deckFixture(state: DeckFixtureState): DeckFixture {
  // `deck:audit-summary` is the Project tab with a summary waiting for
  // Approve: the project is still Drafting and PLAN.md does not exist yet.
  const auditing = state === "deck:audit-summary";
  return {
    state,
    project: auditing ? { ...PROJECT, status: "drafting" } : PROJECT,
    plan: auditing ? { sprints: [] } : state === "deck:plan" ? PLAN_WIDE : PLAN,
    journal: auditing ? JOURNAL.slice(0, 1) : state === "deck:activity" ? JOURNAL_LONG : JOURNAL,
    drafts: DRAFTS,
    status: auditing ? "" : STATUS,
    audit: auditing ? AUDIT : null,
  };
}
