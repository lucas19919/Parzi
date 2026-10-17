<script lang="ts">
  // The column beside a session: where it runs, how full its context is,
  // what changed in its folder and the helpers it started. Read at a
  // glance; the few things to click open what they name.
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
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
  // The server's name when the session runs there.
  export let remoteHost = "";
  export let contextUsed = 0;
  export let contextLimit = 0;
  export let compacting = false;
  export let streaming = false;
  // The agent's name when it compacts on its own (no Compact here).
  export let selfCompacts = "";
  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let approvals: Approval[] = [];
  export let questions: Question[] = [];
  // The plan checklist is in the thread already.
  export let planInline = false;

  const dispatch = createEventDispatcher<{ openSession: { id: string }; openUrl: { url: string }; compact: void; plan: void }>();

  $: pct = contextLimit > 0 ? Math.min(100, Math.round((contextUsed / contextLimit) * 100)) : 0;

  function k(n: number) {
    if (n >= 1_000_000) return `${+(n / 1_000_000).toFixed(1)}M`;
    if (n >= 1_000) return `${Math.round(n / 1_000)}k`;
    return String(n);
  }

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

  $: plan = $planState.project === project ? $planState.plan : null;
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
  <section>
    <h3>Environment</h3>
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
  </section>

  {#if contextLimit > 0}
    <section>
      <h3>Context <span class="aside">{pct}%</span></h3>
      <div class="bar" class:warn={pct >= 70} class:bad={pct >= 90}><i style:width="{pct}%" /></div>
      <div class="line quiet">
        <span class="v">{k(contextUsed)} of {k(contextLimit)}</span>
        {#if !selfCompacts}
          <button class="link" disabled={compacting || streaming} on:click={() => dispatch("compact")}>{compacting ? "Compacting…" : "Compact"}</button>
        {/if}
      </div>
    </section>
  {/if}

  {#if git}
    <section>
      <h3>Changes</h3>
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
    </section>
  {/if}

  {#if plan && planCount && !planInline}
    <section>
      <h3>Plan</h3>
      <button class="line row plan-row" on:click={() => dispatch("plan")}>
        <Icon name="plan" size={12} />
        <span class="v">{plan.title || "Plan"}</span>
        <span class="aside">{planCount.done} of {planCount.total}</span>
      </button>
    </section>
  {/if}

  {#if helpers.length}
    <section>
      <h3>Helpers</h3>
      {#each helpers as h (h.id)}
        {@const tone = sessionTone(h, running, waiting)}
        <button class="line row" use:tooltip={TONE_WORDS[tone]} on:click={() => dispatch("openSession", { id: h.id })}>
          <i class="dot t-{tone}" />
          <span class="v">{h.title || "Untitled"}</span>
        </button>
      {/each}
    </section>
  {/if}
</aside>

<style>
  .info {
    flex: none;
    width: 240px;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding: 16px 14px 24px;
    overflow-y: auto;
    border-left: 1px solid var(--line);
  }
  section {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  h3 {
    display: flex;
    align-items: baseline;
    margin: 0 0 2px;
    font-size: var(--fs-md);
    font-weight: 600;
    color: var(--muted);
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
  .aside {
    margin-left: auto;
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }
  .bar {
    height: 4px;
    margin: 4px 0 2px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--line);
  }
  .bar i {
    display: block;
    height: 100%;
    background: var(--accent);
  }
  .bar.warn i {
    background: var(--warn);
  }
  .bar.bad i {
    background: var(--bad);
  }
  .link {
    flex: none;
    padding: 0;
    background: none;
    border: none;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-sm);
    text-decoration: underline;
    text-underline-offset: 2px;
    cursor: pointer;
  }
  .link:hover:not(:disabled) {
    color: var(--text);
  }
  .link:disabled {
    cursor: default;
    opacity: 0.5;
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
