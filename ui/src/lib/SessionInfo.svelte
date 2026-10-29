<script lang="ts">
  // Column beside a session: where it runs, context, folder changes, helpers.
  import { createEventDispatcher, onMount } from "svelte";
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import ContextBreakdown from "./ContextBreakdown.svelte";
  import JobsCard from "./JobsCard.svelte";
  import NotificationsCard from "./NotificationsCard.svelte";
  import { board } from "./providerStore";
  import { nameOf } from "./providerRows";
  import { api, onRunEvent, type GitStatus, type Question, type SessionMeta } from "./api";
  import type { Approval } from "./live";
  import { planState } from "./planStore";
  import { isTrunk, progress, deriveItems, sessionTone, TONE_WORDS } from "./planModel";
  import { activity } from "./activity";
  import { folderName } from "./tabs";
  import { tooltip } from "./tooltip";
  import { mdHtml, richReady } from "./md";

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
  // A markdown document pinned on top of the column. Null when nothing is pinned.
  export let doc: { title: string; content: string; version: number } | null = null;
  // The column width, owned by App (persisted there). The grip dispatches
  // resize while dragging.
  export let width = 264;

  const dispatch = createEventDispatcher<{
    openSession: { id: string };
    openUrl: { url: string };
    compact: void;
    plan: void;
    openDoc: void;
    closeDoc: void;
    resize: { width: number };
  }>();

  // Drag the left edge to resize. Pointer capture keeps the drag alive
  // outside the grip; clamp keeps the thread readable on both ends.
  let dragging = false;
  function startResize(e: PointerEvent) {
    dragging = true;
    (e.currentTarget as HTMLElement).setPointerCapture?.(e.pointerId);
    e.preventDefault();
  }
  function onResize(e: PointerEvent) {
    if (!dragging) return;
    const aside = (e.currentTarget as HTMLElement).closest(".info") as HTMLElement | null;
    const rect = aside?.getBoundingClientRect();
    // Fall back to window math when the aside isn't laid out yet.
    const next = rect ? rect.right - e.clientX : window.innerWidth - e.clientX - 16;
    dispatch("resize", { width: Math.round(Math.min(520, Math.max(220, next))) });
  }
  function endResize() {
    dragging = false;
  }

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
  $: planError = $planState.project === project ? $planState.error : "";
  $: limits = $board.find((b) => b.provider === provider)?.usage ?? [];
  const kTok = (n: number) => (n >= 1_000_000 ? `${+(n / 1_000_000).toFixed(1)}M` : n >= 1_000 ? `${+(n / 1_000).toFixed(1)}k` : String(n));
  $: planCount = plan ? progress(deriveItems(plan, { project, threads, running, waiting: new Map(), prs: $planState.prs, activity: $activity })) : null;
  $: planAside = planError ? "Unreadable" : planInline ? "Shown" : planCount ? `${planCount.done} of ${planCount.total}` : "";

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

<aside class="info" aria-label="Session" style="width: {width}px" transition:fly={{ x: 28, duration: 220, easing: cubicOut }}>
  <div
    class="grip"
    role="separator"
    aria-orientation="vertical"
    aria-label="Resize session column"
    title="Drag to resize"
    on:pointerdown={startResize}
    on:pointermove={onResize}
    on:pointerup={endResize}
    on:pointercancel={endResize}
  />
  <NotificationsCard on:openSession />
  {#if doc}
    <InfoCard id="doc" title={doc.version > 1 ? `${doc.title} · v${doc.version}` : doc.title}>
      <svelte:fragment slot="aside">
        <span class="doc-acts">
          <button class="doc-btn" use:tooltip={"Open in its own tab"} aria-label="Open document in its own tab" on:click={() => dispatch("openDoc")}><Icon name="external" size={8} /></button>
          <button class="doc-btn" use:tooltip={"Unpin"} aria-label="Unpin document" on:click={() => dispatch("closeDoc")}><Icon name="close" size={8} /></button>
        </span>
      </svelte:fragment>
      {#key `${doc.title}:${doc.version}`}
        <div class="doc msg">{@html mdHtml(doc.content, $richReady)}</div>
      {/key}
    </InfoCard>
  {/if}
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
        {folder}
        {compacting}
        {streaming}
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
          <Icon name="pr" size={8} />
          <span class="v">PR #{pr.number}</span>
          {#if pr.checks !== "none"}
            <span class="checks {pr.checks}"><Icon name={pr.checks === "pass" ? "checkCircle" : pr.checks === "fail" ? "xCircle" : "clock"} size={8} /></span>
          {/if}
        </button>
      {/if}
    </InfoCard>
  {/if}

  {#if (plan && planCount) || planError}
    <InfoCard id="plan" title="Project plan">
      <svelte:fragment slot="aside">{planAside}</svelte:fragment>
      {#if plan && planCount}
        <button class="line row plan-row" use:tooltip={planInline ? "Hide the plan checklist" : "Show the plan checklist"} on:click={() => dispatch("plan")}>
          <Icon name="plan" size={8} />
          <span class="v">{planInline ? "Hide plan" : plan.title || "Plan"}</span>
        </button>
      {:else}
        <button class="line row plan-row" use:tooltip={"Show why the plan can't be read"} on:click={() => dispatch("plan")}>
          <Icon name="plan" size={8} />
          <span class="v">Plan unreadable</span>
        </button>
      {/if}
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
  /* Floating cards beside the session. Width is inline; the grip drags the left edge. */
  .info {
    position: relative;
    flex: none;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    margin: 0 var(--gap) 0 0;
    padding-bottom: calc(var(--space) * 3);
    overflow-y: auto;
    scrollbar-width: none;
  }
  /* The hit area fills the gutter, so the grab line sits in the middle of the gap. */
  .grip {
    position: absolute;
    left: calc(-1 * var(--gap));
    top: 0;
    bottom: 0;
    width: var(--gap);
    cursor: ew-resize;
    touch-action: none;
  }
  .grip::after {
    content: "";
    position: absolute;
    left: 3px;
    top: 12px;
    bottom: 12px;
    width: 2px;
    border-radius: var(--radius-sm);
    background: transparent;
    transition: background 120ms ease;
  }
  .grip:hover::after {
    background: color-mix(in srgb, var(--text) 18%, transparent);
  }
  .doc-acts {
    display: inline-flex;
    gap: var(--space);
  }
  .doc-btn {
    flex: none;
    width: calc(var(--space) * 6);
    height: calc(var(--space) * 6);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    background: none;
    border: none;
    border-radius: var(--radius-sm);
    color: var(--muted);
    cursor: pointer;
  }
  .doc-btn:hover {
    background: color-mix(in srgb, var(--text) 8%, transparent);
    color: var(--text);
  }
  .doc {
    min-height: 0;
    max-height: 46vh;
    overflow-y: auto;
    padding: 0;
    font-size: var(--fs-md);
    line-height: 1.55;
    overflow-wrap: anywhere;
  }
  .line {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-height: calc(var(--space) * 4);
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
    width: 100%;
    margin: 0;
    padding: calc(var(--space) * 2);
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
    width: calc(var(--space) * 2);
    height: calc(var(--space) * 2);
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
