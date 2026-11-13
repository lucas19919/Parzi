<script lang="ts">
  import { tokenCount } from "./format";
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard, { unfold } from "./InfoCard.svelte";
  import ContextBreakdown from "./ContextBreakdown.svelte";
  import RunningCard from "./RunningCard.svelte";
  import FilesCard from "./FilesCard.svelte";
  import type { ArtifactDoc } from "./tabs";
  import HostedCard from "./HostedCard.svelte";
  import { hostedSites, clearHosted } from "./hosted";
  import TodoList, { todoItems, todoOpen } from "./TodoList.svelte";
  import { dockedTodos, setTodoDocked } from "./todoDock";
  import { board, reloadBoard } from "./providerStore";
  import ResizeGrip from "./ResizeGrip.svelte";
  import { nameOf } from "./providerRows";
  import { api, onRunEvent, type GitStatus, type Question, type SessionMeta } from "./api";
  import type { Approval } from "./live";
  import { planState } from "./planStore";
  import { isTrunk, progress, deriveItems, groupItems } from "./planModel";
  import PlanMark from "./PlanMark.svelte";
  import { activity } from "./activity";
  import { folderName } from "./tabs";
  import { tooltip } from "./tooltip";

  export let session = "";
  export let project = "";
  export let projectTitle = "";
  export let folder = "";
  export let branch = "";
  export let remoteHost = "";
  export let contextUsed = 0;
  export let contextLimit = 0;
  export let compacting = false;
  export let streaming = false;
  export let tokensIn = 0;
  export let tokensOut = 0;
  export let costUsd = 0;
  export let provider = "";
  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let approvals: Approval[] = [];
  export let questions: Question[] = [];
  export let planInline = false;
  export let todo: unknown = null;
  $: stepsHere = !!todo && $dockedTodos.has(session) && todoOpen(todo);
  $: if (stepsHere) unfold("steps");
  $: stepItems = todoItems(todo);
  export let width = 264;
  // Card order: environment first, then the plan. Swappable by drag,
  // persisted per machine.
  const ORDER_KEY = "parzi.info.order";
  const DEFAULT_ORDER = ["env", "plan", "steps", "context", "usage", "changes", "files", "running"];
  function loadOrder(): string[] {
    try {
      const v = JSON.parse(localStorage.getItem(ORDER_KEY) ?? "[]");
      if (Array.isArray(v)) {
        const known = v.filter((x) => typeof x === "string" && DEFAULT_ORDER.includes(x));
        return [...known, ...DEFAULT_ORDER.filter((x) => !known.includes(x))];
      }
    } catch {}
    return [...DEFAULT_ORDER];
  }
  let cardOrder: string[] = loadOrder();
  $: pos = (id: string) => {
    const i = cardOrder.indexOf(id);
    return i < 0 ? 99 : i;
  };
  function saveOrder() {
    try {
      localStorage.setItem(ORDER_KEY, JSON.stringify(cardOrder));
    } catch {}
  }
  let dragId = "";
  function onDragStart(id: string, e: DragEvent) {
    dragId = id;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = "move";
      try {
        e.dataTransfer.setData("text/plain", id);
      } catch {}
    }
  }
  function onDrop(target: string, e: DragEvent) {
    e.preventDefault();
    const from = dragId || (() => {
      try {
        return e.dataTransfer?.getData("text/plain") ?? "";
      } catch {
        return "";
      }
    })();
    dragId = "";
    if (!from || from === target) return;
    const next = [...cardOrder];
    const fi = next.indexOf(from);
    const ti = next.indexOf(target);
    if (fi < 0 || ti < 0) return;
    next.splice(ti, 0, next.splice(fi, 1)[0]);
    cardOrder = next;
    saveOrder();
  }
  // Open or shut: the column animates its own width instead of mounting
  // in and out, so the thread never jumps.
  export let open = true;
  /** The file open in the side bar's second column, if any. */
  export let reading = "";

  const dispatch = createEventDispatcher<{
    openSession: { id: string };
    openUrl: { url: string };
    compact: void;
    plan: void;
    resize: { width: number };
    openFile: ArtifactDoc;
  }>();

  $: pct = contextLimit > 0 ? Math.min(100, Math.round((contextUsed / contextLimit) * 100)) : 0;

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
  $: planError = $planState.project === project ? $planState.error : "";
  $: limits = $board.find((b) => b.provider === provider)?.usage ?? [];
  $: site = $hostedSites[session];
  $: planViews = plan ? deriveItems(plan, { project, threads, running, waiting: new Map(), prs: $planState.prs, activity: $activity }) : [];
  $: planCount = plan ? progress(planViews) : null;
  $: planAside = planError ? "Unreadable" : planCount ? `${planCount.done} of ${planCount.total}` : "";
  // The open items in plan order, the first few; the thread shows them all.
  const PLAN_ROWS = 6;
  $: planOpen = plan ? groupItems(plan, planViews).open.flatMap((g) => g.items) : [];

  onMount(() => {
    const onFocus = () => void loadGit();
    window.addEventListener("focus", onFocus);
    const un = onRunEvent((e) => {
      if (e.kind === "done" || e.kind === "error" || e.kind === "paused") {
        void loadGit();
        void reloadBoard();
      }
    });
    return () => {
      window.removeEventListener("focus", onFocus);
      un.then((f) => f()).catch(() => {});
    };
  });
</script>

<aside class="info" class:shut={!open} aria-label="Session" style="width: {open ? width : 0}px" inert={!open}>
  <ResizeGrip label="Resize session column" min={220} max={520} on:resize />
  <div class="col">
  <div class="slot" style="order: {pos('env')}" draggable="true" on:dragstart={(e) => onDragStart('env', e)} on:dragover={(e) => e.preventDefault()} on:drop={(e) => onDrop('env', e)}>
  <InfoCard id="env" title="Environment" foldedAside>
    <svelte:fragment slot="aside">{remoteHost ? "Remote" : "Local"}</svelte:fragment>
    <div class="info-line">
      <Icon name={remoteHost ? "server" : "desktop"} size={12} />
      <span class="info-v">{remoteHost || "This computer"}</span>
    </div>
    <div class="info-line" title={folder}>
      <Icon name={project ? "project" : "folder"} size={12} />
      <span class="info-v">{projectTitle || (folder ? folderName(folder) : "No folder")}</span>
    </div>
    {#if branch}
      <div class="info-line">
        <Icon name="branch" size={12} />
        <span class="info-v mono">{branch}</span>
      </div>
    {/if}
  </InfoCard>
  </div>

  <!-- Pinned second: whatever the agent hosts stays one glance down. -->
  {#if site}
  <div class="slot" style="order: {pos('env')}">
    <HostedCard {site} on:close={() => clearHosted(session)} />
  </div>
  {/if}

  <!-- While the plan is open in the thread, the column does not repeat it. -->
  {#if ((plan && planCount) || planError) && !planInline}
  <div class="slot" style="order: {pos('plan')}" draggable="true" on:dragstart={(e) => onDragStart('plan', e)} on:dragover={(e) => e.preventDefault()} on:drop={(e) => onDrop('plan', e)}>
    <InfoCard id="plan" title={plan?.title ?? "Plan"}>
      <svelte:fragment slot="aside">{planAside}</svelte:fragment>
      {#if plan && planCount}
        {#each planOpen.slice(0, PLAN_ROWS) as v (v.item.id)}
          <button class="info-line row plan-item" use:tooltip={v.item.detail || "Show the plan in the thread"} on:click={() => dispatch("plan")}>
            <PlanMark state={v.state} />
            <span class="info-v">{v.item.title}</span>
          </button>
        {:else}
          <div class="info-line quiet">Everything is done</div>
        {/each}
        {#if planOpen.length > PLAN_ROWS}
          <div class="info-line quiet">and {planOpen.length - PLAN_ROWS} more</div>
        {/if}
        <button class="info-line row plan-row" on:click={() => dispatch("plan")}>
          <Icon name="plan" size={12} />
          <span class="info-v">Show in the thread</span>
        </button>
      {:else}
        <button class="info-line row plan-row" use:tooltip={"Show why the plan can't be read"} on:click={() => dispatch("plan")}>
          <Icon name="plan" size={12} />
          <span class="info-v">Plan unreadable</span>
        </button>
      {/if}
    </InfoCard>
  </div>
  {/if}

  {#if stepsHere}
  <div class="slot" style="order: {pos('steps')}" draggable="true" on:dragstart={(e) => onDragStart('steps', e)} on:dragover={(e) => e.preventDefault()} on:drop={(e) => onDrop('steps', e)}>
    <InfoCard id="steps" title="Steps">
      <svelte:fragment slot="aside">{stepItems.filter((i) => i.state === "done").length} of {stepItems.length}</svelte:fragment>
      <svelte:fragment slot="actions">
        <button class="doc-btn" use:tooltip={"Move to the thread"} aria-label="Move steps to the thread" on:click={() => setTodoDocked(session, false)}><Icon name="external" size={12} /></button>
      </svelte:fragment>
      <TodoList {todo} working={streaming} docked />
    </InfoCard>
  </div>
  {/if}

  {#if contextLimit > 0}
  <div class="slot" style="order: {pos('context')}" draggable="true" on:dragstart={(e) => onDragStart('context', e)} on:dragover={(e) => e.preventDefault()} on:drop={(e) => onDrop('context', e)}>
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
  </div>
  {/if}
  {#if limits.length || (!contextLimit && (tokensIn || tokensOut))}
  <div class="slot" style="order: {pos('usage')}" draggable="true" on:dragstart={(e) => onDragStart('usage', e)} on:dragover={(e) => e.preventDefault()} on:drop={(e) => onDrop('usage', e)}>
    <InfoCard id="usage" title="Usage">
      <svelte:fragment slot="aside">{costUsd > 0 ? `$${costUsd.toFixed(2)}` : ""}</svelte:fragment>
      {#if !contextLimit && (tokensIn || tokensOut)}
        <div class="info-line"><span class="info-v">{tokenCount(tokensIn)} in · {tokenCount(tokensOut)} out</span></div>
      {/if}
      {#each limits as w (w.label)}
        <div class="info-line quiet">
          <span class="info-v">{nameOf(provider)} {w.label}</span>
          <span class="pct">{Math.round(w.used_percent)}%</span>
        </div>
      {/each}
    </InfoCard>
  </div>
  {/if}

  {#if git}
  <div class="slot" style="order: {pos('changes')}" draggable="true" on:dragstart={(e) => onDragStart('changes', e)} on:dragover={(e) => e.preventDefault()} on:drop={(e) => onDrop('changes', e)}>
    <InfoCard id="changes" title="Changes">
      <svelte:fragment slot="aside">
        {#if git.added || git.removed}
          <span class="add">+{git.added.toLocaleString()}</span>&nbsp;<span class="del">−{git.removed.toLocaleString()}</span>
        {/if}
      </svelte:fragment>
      <div class="info-line">
        <span class="info-v" class:quiet={!git.changed && !git.untracked}>
          {git.changed || git.untracked ? [git.changed ? `${git.changed} changed` : "", git.untracked ? `${git.untracked} new` : ""].filter(Boolean).join(" · ") : "No changes"}
        </span>
      </div>
      {#if git.upstream && (git.ahead || git.behind)}
        <div class="info-line quiet"><span class="info-v">{[git.ahead ? `${git.ahead} ahead` : "", git.behind ? `${git.behind} behind` : ""].filter(Boolean).join(" · ")}</span></div>
      {/if}
      {#if pr}
        <button class="info-line row" use:tooltip={pr.title} on:click={() => pr && dispatch("openUrl", { url: pr.url })}>
          <Icon name="pr" size={12} />
          <span class="info-v">PR #{pr.number}</span>
          {#if pr.checks !== "none"}
            <span class="checks {pr.checks}"><Icon name={pr.checks === "pass" ? "checkCircle" : pr.checks === "fail" ? "xCircle" : "clock"} size={12} /></span>
          {/if}
        </button>
      {/if}
    </InfoCard>
  </div>
  {/if}

  <div class="slot" style="order: {pos('files')}" draggable="true" on:dragstart={(e) => onDragStart('files', e)} on:dragover={(e) => e.preventDefault()} on:drop={(e) => onDrop('files', e)}>
  <FilesCard {folder} shown={open} current={reading} on:open={(e) => dispatch("openFile", e.detail)} />
  </div>

  <div class="slot" style="order: {pos('running')}" draggable="true" on:dragstart={(e) => onDragStart('running', e)} on:dragover={(e) => e.preventDefault()} on:drop={(e) => onDrop('running', e)}>
  <RunningCard {session} {helpers} {running} {waiting} on:openSession />
  </div>
  </div>
</aside>

<style>
  /* The column itself must not clip: the resize grip sits just left of it. */
  .info {
    position: relative;
    flex: none;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    margin: 0 var(--gap) 0 0;
    transition: width 300ms var(--ease-out), margin-right 300ms var(--ease-out);
  }
  .col {
    flex: 1;
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: calc(var(--space) * 2);
    padding-bottom: calc(var(--space) * 3);
    overflow-x: hidden;
    overflow-y: auto;
    scrollbar-width: none;
  }
  .info.shut {
    margin-right: 0;
  }
  .slot {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .slot[draggable="true"] {
    cursor: grab;
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
    border-radius: var(--radius);
    color: var(--muted);
    cursor: pointer;
  }
  .doc-btn:hover {
    background: color-mix(in srgb, var(--text) 8%, transparent);
    color: var(--text);
  }
  .pct {
    flex: none;
    font-variant-numeric: tabular-nums;
  }
  .add {
    color: var(--ok-text);
  }
  .del {
    color: var(--bad-text);
  }
  .checks.pass :global(svg) {
    color: var(--ok-text);
  }
  .checks.fail :global(svg) {
    color: var(--bad-text);
  }
  .plan-item :global(.mark) {
    width: 12px;
    height: 12px;
  }
</style>
