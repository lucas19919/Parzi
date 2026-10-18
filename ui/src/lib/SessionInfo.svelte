<script lang="ts">
  // The column beside a session: where it runs, how full its context is,
  // what changed in its folder and the helpers it started. Read at a
  // glance; the few things to click open what they name.
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import ContextBreakdown from "./ContextBreakdown.svelte";
  import JobsCard from "./JobsCard.svelte";
  import { board } from "./providerStore";
  import { nameOf } from "./providerRows";
  import { api, onRunEvent, type GitStatus, type Question, type SessionMeta } from "./api";
  import type { Approval } from "./live";
  import { planState } from "./planStore";
  import { isTrunk, progress, deriveItems, sessionTone, TONE_WORDS } from "./planModel";
  import { activity } from "./activity";
  import { folderName } from "./tabs";
  import { tooltip } from "./tooltip";

  export let session = "";
  export let project = "";
  export let projectTitle = "";
  export let folder = "";
  export let branch = "";
  export let lane = "";
  // The server's name when the session runs there.
  export let remoteHost = "";
  export let contextUsed = 0;
  export let contextLimit = 0;
  export let compacting = false;
  export let streaming = false;
  // The agent's name when it compacts on its own (no Compact here).
  export let selfCompacts = "";
  // What the session spent, for agents that report no context window, and
  // the agent whose plan limits apply.
  export let tokensIn = 0;
  export let tokensOut = 0;
  export let costUsd = 0;
  export let provider = "";
  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let approvals: Approval[] = [];
  export let questions: Question[] = [];
  // The plan checklist is in the thread already.
  export let planInline = false;

  const dispatch = createEventDispatcher<{ openSession: { id: string }; openUrl: { url: string }; compact: void; plan: void }>();

  $: pct = contextLimit > 0 ? Math.min(100, Math.round((contextUsed / contextLimit) * 100)) : 0;

  // Git, for a folder on this computer.
  let git: GitStatus | null = null;
  let gitFor: string | null = null;
  $: gitDir = remoteHost ? "" : folder;
  $: if (gitDir !== gitFor) {
    gitFor = gitDir;
    git = null;
    void loadGit();
  }
  async function loadGit() {
    const dir = gitFor;
    const s = dir ? await api.gitStatus(dir).catch(() => null) : null;
    if (dir === gitFor) git = s;
  }
  $: pr = branch && !isTrunk(branch) ? ($planState.prs.find((p) => p.head === branch && p.state === "open") ?? null) : null;

  // Helpers: every session this one started, and theirs.
  $: waiting = new Set([...approvals.map((a) => a.session), ...questions.map((q) => q.session)]);
  $: helpers = (() => {
    const out: SessionMeta[] = [];
    const seen = new Set([session]);
    let frontier = [session];
    for (let depth = 0; frontier.length && depth < 6; depth++) {
      const next = threads.filter((t) => t.parent_id && frontier.includes(t.parent_id) && !seen.has(t.id));
      for (const t of next) seen.add(t.id);
      out.push(...next);
      frontier = next.map((t) => t.id);
    }
    return out.sort((a, b) => (Date.parse(b.updated) || 0) - (Date.parse(a.updated) || 0)).slice(0, 8);
  })();
  $: busy = helpers.filter((h) => running.has(h.id)).length;

  $: plan = $planState.project === project ? $planState.plan : null;
  $: limits = $board.find((b) => b.provider === provider)?.usage ?? [];
  const kTok = (n: number) => (n >= 1_000_000 ? `${+(n / 1_000_000).toFixed(1)}M` : n >= 1_000 ? `${+(n / 1_000).toFixed(1)}k` : String(n));
  $: planCount = plan ? progress(deriveItems(plan, { project, threads, running, waiting: new Map(), prs: $planState.prs, activity: $activity })) : null;

  onMount(() => {
    const onFocus = () => void loadGit();
    window.addEventListener("focus", onFocus);
    const un = onRunEvent((e) => {
      if (e.kind === "done" || e.kind === "error" || e.kind === "paused") void loadGit();
    });
    return () => {
      window.removeEventListener("focus", onFocus);
      un.then((f) => f()).catch(() => {});
    };
  });
</script>

<aside class="info" aria-label="Session">
  <InfoCard id="env" title="Environment" foldedAside>
    <svelte:fragment slot="aside">{remoteHost ? "Remote" : "Local"}</svelte:fragment>
    <div class="line">
      <Icon name={remoteHost ? "server" : "desktop"} size={12} />
      <span class="v">{remoteHost || "This computer"}</span>
    </div>
    <div class="line" title={folder}>
      <Icon name={project ? "project" : "folder"} size={12} />
      <span class="v">{projectTitle || (folder ? folderName(folder) : "No folder")}</span>
    </div>
    {#if branch}
      <div class="line">
        <Icon name="branch" size={12} />
        <span class="v mono">{branch}</span>
      </div>
    {/if}
  </InfoCard>

  {#if contextLimit > 0}
    <InfoCard id="context" title="Context" foldedAside>
      <svelte:fragment slot="aside">{pct}%</svelte:fragment>
      <ContextBreakdown
        used={contextUsed}
        limit={contextLimit}
        {lane}
        {folder}
        {compacting}
        {streaming}
        {selfCompacts}
        on:compact={() => dispatch("compact")}
      />
    </InfoCard>
  {:else if tokensIn || tokensOut || limits.length}
    <!-- No context window from this agent: what the session spent. -->
    <InfoCard id="usage" title="Usage">
      <svelte:fragment slot="aside">{costUsd > 0 ? `$${costUsd.toFixed(2)}` : ""}</svelte:fragment>
      {#if tokensIn || tokensOut}
        <div class="line"><span class="v">{kTok(tokensIn)} in · {kTok(tokensOut)} out</span></div>
      {/if}
      {#each limits as w (w.label)}
        <div class="line quiet">
          <span class="v">{nameOf(provider)} {w.label}</span>
          <span class="pct">{Math.round(w.used_percent)}%</span>
        </div>
      {/each}
    </InfoCard>
  {/if}

  {#if git}
    <InfoCard id="changes" title="Changes">
      <svelte:fragment slot="aside">
        {#if git.added || git.removed}
          <span class="add">+{git.added.toLocaleString()}</span>&nbsp;<span class="del">−{git.removed.toLocaleString()}</span>
        {/if}
      </svelte:fragment>
      <div class="line">
        <span class="v" class:quiet={!git.changed && !git.untracked}>
          {git.changed || git.untracked ? [git.changed ? `${git.changed} changed` : "", git.untracked ? `${git.untracked} new` : ""].filter(Boolean).join(" · ") : "No changes"}
        </span>
      </div>
      {#if git.upstream && (git.ahead || git.behind)}
        <div class="line quiet"><span class="v">{[git.ahead ? `${git.ahead} ahead` : "", git.behind ? `${git.behind} behind` : ""].filter(Boolean).join(" · ")}</span></div>
      {/if}
      {#if pr}
        <button class="line row" use:tooltip={pr.title} on:click={() => pr && dispatch("openUrl", { url: pr.url })}>
          <Icon name="pr" size={12} />
          <span class="v">PR #{pr.number}</span>
          {#if pr.checks !== "none"}
            <span class="checks {pr.checks}"><Icon name={pr.checks === "pass" ? "checkCircle" : pr.checks === "fail" ? "xCircle" : "clock"} size={12} /></span>
          {/if}
        </button>
      {/if}
    </InfoCard>
  {/if}

  {#if plan && planCount && !planInline}
    <InfoCard id="plan" title="Project plan">
      <svelte:fragment slot="aside">{planCount.done} of {planCount.total}</svelte:fragment>
      <button class="line row plan-row" on:click={() => dispatch("plan")}>
        <Icon name="plan" size={12} />
        <span class="v">{plan.title || "Plan"}</span>
      </button>
    </InfoCard>
  {/if}

  <JobsCard {session} />

  {#if helpers.length}
    <InfoCard id="helpers" title="Side tasks">
      <svelte:fragment slot="aside">{busy ? `${busy} running` : helpers.length}</svelte:fragment>
      {#each helpers as h (h.id)}
        {@const tone = sessionTone(h, running, waiting)}
        <button class="line row" use:tooltip={TONE_WORDS[tone]} on:click={() => dispatch("openSession", { id: h.id })}>
          <i class="dot t-{tone}" />
          <span class="v">{h.title || "Untitled"}</span>
        </button>
      {/each}
    </InfoCard>
  {/if}
</aside>

<style>
  /* Cards that float beside the session, not a panel inside it. */
  .info {
    flex: none;
    width: 264px;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0 8px 0 0;
    padding-bottom: 12px;
    overflow-y: auto;
    scrollbar-width: none;
    animation: enter 600ms var(--ease-out);
  }
  @keyframes enter {
    from {
      opacity: 0;
      transform: translateX(8px);
    }
  }
  .line {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 24px;
    color: var(--text);
    font-size: var(--fs-sm);
  }
  .line :global(svg) {
    flex: none;
    color: var(--muted);
  }
  .line.quiet,
  .quiet {
    color: var(--muted);
  }
  .row {
    width: calc(100% + 12px);
    margin: 0 -6px;
    padding: 0 6px;
    background: none;
    border: none;
    border-radius: var(--radius);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .row:hover {
    background: color-mix(in srgb, var(--text) 6%, transparent);
  }
  .v {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .mono {
    font-family: var(--mono);
    font-size: var(--fs-xs);
  }
  /* Lightened toward the text so small numbers stay readable on a card. */
  .pct {
    flex: none;
    font-variant-numeric: tabular-nums;
  }
  .add {
    color: color-mix(in srgb, var(--ok) 75%, var(--text));
  }
  .del {
    color: color-mix(in srgb, var(--bad) 62%, var(--text));
  }
  .checks.pass :global(svg) {
    color: var(--ok);
  }
  .checks.fail :global(svg) {
    color: var(--bad);
  }
  .dot {
    width: 7px;
    height: 7px;
    flex: none;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.t-live {
    background: var(--accent);
  }
  .dot.t-need {
    background: var(--warn);
  }
  .dot.t-bad {
    background: var(--bad);
  }
  .dot.t-ok {
    background: var(--ok);
  }
</style>
