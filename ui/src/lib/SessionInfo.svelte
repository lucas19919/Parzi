<script lang="ts">
  import { resetsIn, tokenCount } from "./format";
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import ContextBreakdown from "./ContextBreakdown.svelte";
  import RunningCard from "./RunningCard.svelte";
  import { board, refreshUsage, reloadBoard } from "./providerStore";
  import ResizeGrip from "./ResizeGrip.svelte";
  import { nameOf } from "./providerRows";
  import { api, onRunEvent, type Question, type SessionMeta } from "./api";
  import type { Approval } from "./live";
  import { folderName } from "./tabs";
  import ProjectMenu from "./ProjectMenu.svelte";
  import { toast, toastError } from "./toast";
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
  export let width = 264;
  // Open or shut: the column animates its own width instead of mounting
  // in and out, so the thread never jumps.
  export let open = true;

  const dispatch = createEventDispatcher<{
    openSession: { id: string };
    compact: void;
    resize: { width: number };
    moved: { meta: SessionMeta };
  }>();

  // Where the session runs can change while it is stopped: another project,
  // or another folder of this one (a worktree, which brings its branch).
  $: local = !remoteHost && !/^[a-z]:/.test(session);
  $: stuck = !session ? "" : !local ? "Moving works on this computer's sessions for now" : streaming ? "Stop the run to move the session" : "";
  $: movable = !!session && !stuck;
  let projBtn: HTMLButtonElement | null = null;
  let projOpen = false;
  let moving = false;

  async function move(cwd: string, slug?: string) {
    if (!cwd || moving || !movable) return;
    moving = true;
    try {
      const meta = await api.setSessionEnvironment(session, cwd, slug ?? project);
      dispatch("moved", { meta });
      toast(`Moved to ${folderName(meta.cwd || cwd)}`);
    } catch (e) {
      toastError(e);
    } finally {
      moving = false;
    }
  }

  async function pickFolder() {
    if (!movable || moving) return;
    try {
      const picked = await api.pickFolder(folder);
      if (picked) await move(picked);
    } catch (e) {
      toastError(e);
    }
  }

  $: pct = contextLimit > 0 ? Math.min(100, Math.round((contextUsed / contextLimit) * 100)) : 0;

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

  $: limits = $board.find((b) => b.provider === provider)?.usage ?? [];
  $: agent = provider ? nameOf(provider) : "";
  /** Agents whose plan limits Parzi can read (their /usage or /status). */
  const READS_LIMITS = new Set(["claude", "codex"]);
  // How the agent bills, from Settings: a plan shows its limits, a key its tokens and cost.
  let billing: Record<string, string> = {};
  $: onKey = billing[provider] === "api_key";
  async function loadBilling() {
    try {
      const cfg = await api.getConfig();
      billing = Object.fromEntries(Object.entries(cfg.providers ?? {}).map(([id, e]) => [id, e.billing ?? "subscription"]));
    } catch {
      billing = {};
    }
  }

  // The Usage card shows the plan's real limits: read from each agent's own
  // source when the column shows, then every 5 minutes while it stays open.
  $: if (open) void refreshUsage();
  $: if (open) void loadBilling();
  onMount(() => {
    const un = onRunEvent((e) => {
      if (e.kind === "done" || e.kind === "error" || e.kind === "paused") void reloadBoard();
    });
    const every = setInterval(() => {
      if (open && !document.hidden) void refreshUsage();
    }, 5 * 60_000);
    return () => {
      clearInterval(every);
      un.then((f) => f()).catch(() => {});
    };
  });
</script>

<aside class="info" class:shut={!open} aria-label="Session" style="width: {open ? width : 0}px" inert={!open}>
  <ResizeGrip label="Resize session column" min={220} max={520} on:resize />
  <div class="col">
  <InfoCard id="env" title="Environment" foldedAside>
    <svelte:fragment slot="aside">{remoteHost ? "Remote" : "Local"}</svelte:fragment>
    <div class="info-line">
      <Icon name={remoteHost ? "server" : "desktop"} size={12} />
      <span class="info-v">{remoteHost || "This computer"}</span>
    </div>
    {#if session}
      <button
        bind:this={projBtn}
        class="info-line row"
        aria-disabled={!movable || moving}
        aria-haspopup="menu"
        aria-expanded={projOpen}
        use:tooltip={stuck || `In ${folder}. Pick another project to move the session there.`}
        on:click={() => movable && !moving && (projOpen = !projOpen)}
      >
        <Icon name={project ? "project" : "folder"} size={12} />
        <span class="info-v">{projectTitle || (folder ? folderName(folder) : "No folder")}</span>
      </button>
      {#if folder}
        <button class="info-line row" aria-disabled={!movable || moving} use:tooltip={stuck || "Move to another folder, such as a worktree on another branch"} on:click={pickFolder}>
          <Icon name="branch" size={12} />
          <span class="info-v mono">{branch || "No branch"}</span>
        </button>
      {/if}
    {:else}
      <div class="info-line" use:tooltip={folder}>
        <Icon name={project ? "project" : "folder"} size={12} />
        <span class="info-v">{projectTitle || (folder ? folderName(folder) : "No folder")}</span>
      </div>
      {#if branch}
        <div class="info-line">
          <Icon name="branch" size={12} />
          <span class="info-v mono">{branch}</span>
        </div>
      {/if}
    {/if}
  </InfoCard>
  {#if projOpen}
    <ProjectMenu
      anchor={projBtn}
      {folder}
      project={project ? { slug: project, title: projectTitle || project } : null}
      {branch}
      none={false}
      on:close={() => (projOpen = false)}
      on:error={(e) => toast(e.detail.text, true)}
      on:project={(e) => e.detail.path && void move(e.detail.path, e.detail.slug)}
    />
  {/if}

  <InfoCard id="context" title="Context" foldedAside>
    <svelte:fragment slot="aside">{contextLimit > 0 ? `${pct}%` : ""}</svelte:fragment>
    {#if contextLimit > 0}
      <ContextBreakdown
        used={contextUsed}
        limit={contextLimit}
        {folder}
        {compacting}
        {streaming}
        on:compact={() => dispatch("compact")}
      />
    {:else}
      <div class="info-line">
        <span class="info-v">Used</span>
        <span class="pct" class:quiet={!contextUsed}>{contextUsed ? tokenCount(contextUsed) : "—"}</span>
      </div>
    {/if}
  </InfoCard>

  <!-- Plan limits as rows; no prose. Claude and Codex show their windows
       empty until read; an agent with no plan limits has no card. -->
  {#if provider && (onKey || limits.length || READS_LIMITS.has(provider))}
    <InfoCard id="usage" title="Usage">
      <svelte:fragment slot="aside">{onKey ? "API key" : agent}</svelte:fragment>
      {#if onKey}
        <div class="info-line"><span class="info-v">Tokens</span><span class="pct">{tokenCount(tokensIn + tokensOut)}</span></div>
        <div class="info-line"><span class="info-v">Cost</span><span class="pct">${costUsd.toFixed(2)}</span></div>
      {:else if limits.length}
        {#each limits as w (w.label)}
          <div class="info-line">
            <span class="info-v">{w.label}</span>
            {#if w.resets_at}<span class="quiet">{resetsIn(w.resets_at)}</span>{/if}
            <span class="pct">{Math.round(w.used_percent)}%</span>
          </div>
        {/each}
      {:else}
        {#each ["Session", "Weekly"] as label (label)}
          <div class="info-line"><span class="info-v">{label}</span><span class="pct quiet">—</span></div>
        {/each}
      {/if}
    </InfoCard>
  {/if}

  <RunningCard {session} {helpers} {running} {waiting} on:openSession />
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
  /* Stopped only: while it runs the line still says where, and why it can't move. */
  button.info-line[aria-disabled="true"] {
    cursor: default;
  }
  button.info-line.row[aria-disabled="true"]:hover {
    background: none;
  }
  .pct {
    flex: none;
    font-variant-numeric: tabular-nums;
  }
</style>
