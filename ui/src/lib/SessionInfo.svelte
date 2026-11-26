<script lang="ts">
  import { tokenCount } from "./format";
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import ContextBreakdown from "./ContextBreakdown.svelte";
  import RunningCard from "./RunningCard.svelte";
  import { reloadBoard } from "./providerStore";
  import ResizeGrip from "./ResizeGrip.svelte";
  import { api, onRunEvent, type Question, type SessionMeta } from "./api";
  import type { Approval } from "./live";
  import { folderName } from "./tabs";
  import type { Notice } from "./turns";
  import ProjectMenu from "./ProjectMenu.svelte";
  import BranchMenu from "./BranchMenu.svelte";
  import { toast, toastError } from "./toast";
  import { tooltip } from "./tooltip";

  export let session = "";
  export let project = "";
  export let projectTitle = "";
  export let folder = "";
  export let branch = "";
  export let remoteHost = "";
  export let remoteAvailable = "";
  export let contextUsed = 0;
  export let contextLimit = 0;
  export let compacting = false;
  export let streaming = false;
  export let threads: SessionMeta[] = [];
  export let running: Set<string> = new Set();
  export let approvals: Approval[] = [];
  export let questions: Question[] = [];
  /** The thread's notices and messages from other sessions. */
  export let notices: Notice[] = [];
  export let width = 264;
  // Open or shut: the column animates its own width instead of mounting
  // in and out, so the thread never jumps.
  export let open = true;

  const dispatch = createEventDispatcher<{
    openSession: { id: string };
    compact: void;
    resize: { width: number };
    moved: { meta: SessionMeta };
    branchChanged: { branch: string };
    toggleRemote: { on: boolean };
  }>();

  // Where the session runs can change while it is stopped: another project,
  // another branch, or a worktree.
  $: local = !remoteHost && !/^[a-z]:/.test(session);
  $: stuck = !session ? "" : !local ? "Moving works on this computer's sessions for now" : streaming ? "Stop the run to switch environment" : "";
  $: movable = !!session && !stuck;
  $: canSwitchBranch = !!folder && local && !streaming;
  $: canToggleHost = !session && !!remoteAvailable && !streaming;
  $: sessionTitle = threads.find((t) => t.id === session)?.title ?? "";
  let projBtn: HTMLButtonElement | null = null;
  let branchBtn: HTMLButtonElement | null = null;
  let projOpen = false;
  let branchOpen = false;
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

  async function switchBranch(target: string) {
    if (!folder || moving || !canSwitchBranch) return;
    moving = true;
    try {
      const next = await api.gitSwitchBranch(folder, target);
      dispatch("branchChanged", { branch: next || target });
      toast(`Switched to ${next || target}`);
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

  onMount(() => {
    const un = onRunEvent((e) => {
      if (e.kind === "done" || e.kind === "error" || e.kind === "paused") void reloadBoard();
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  });
</script>

<aside class="info" class:shut={!open} aria-label="Session" style="width: {open ? width : 0}px" inert={!open}>
  <ResizeGrip label="Resize session column" min={220} max={520} on:resize />
  <div class="col">
  <InfoCard id="env" title="Environment" foldedAside>
    <svelte:fragment slot="aside">{remoteHost ? "Remote" : "Local"}</svelte:fragment>
    {#if canToggleHost}
      <button
        class="info-line row"
        use:tooltip={remoteHost ? `Switch to this computer (currently ${remoteHost})` : `Switch to remote host ${remoteAvailable}`}
        on:click={() => dispatch("toggleRemote", { on: !remoteHost })}
      >
        <Icon name={remoteHost ? "server" : "desktop"} size={12} />
        <span class="info-v">{remoteHost || "This computer"}</span>
        <span class="row-chev" aria-hidden="true"><Icon name="chevDown" size={12} /></span>
      </button>
    {:else}
      <div class="info-line" use:tooltip={remoteAvailable && session ? "Host is fixed once a session starts" : ""}>
        <Icon name={remoteHost ? "server" : "desktop"} size={12} />
        <span class="info-v">{remoteHost || "This computer"}</span>
      </div>
    {/if}
    {#if session}
      <button
        bind:this={projBtn}
        class="info-line row"
        aria-disabled={!movable || moving}
        aria-haspopup="menu"
        aria-expanded={projOpen}
        use:tooltip={stuck || `In ${folder}. Click to switch project.`}
        on:click={() => {
          if (!movable || moving) return;
          branchOpen = false;
          projOpen = !projOpen;
        }}
      >
        <Icon name={project ? "project" : "folder"} size={12} />
        <span class="info-v">{projectTitle || (folder ? folderName(folder) : "No folder")}</span>
        {#if movable}<span class="row-chev" aria-hidden="true"><Icon name="chevDown" size={12} /></span>{/if}
      </button>
      {#if folder}
        <button
          bind:this={branchBtn}
          class="info-line row"
          aria-disabled={!canSwitchBranch || moving}
          aria-haspopup="menu"
          aria-expanded={branchOpen}
          use:tooltip={stuck || "Switch branch, worktree, or folder"}
          on:click={() => {
            if (!canSwitchBranch || moving) return;
            projOpen = false;
            branchOpen = !branchOpen;
          }}
        >
          <Icon name="branch" size={12} />
          <span class="info-v mono">{branch || "No branch"}</span>
          {#if canSwitchBranch}<span class="row-chev" aria-hidden="true"><Icon name="chevDown" size={12} /></span>{/if}
        </button>
      {/if}
    {:else}
      <div class="info-line" use:tooltip={folder}>
        <Icon name={project ? "project" : "folder"} size={12} />
        <span class="info-v">{projectTitle || (folder ? folderName(folder) : "No folder")}</span>
      </div>
      {#if folder}
        <button
          bind:this={branchBtn}
          class="info-line row"
          aria-disabled={!canSwitchBranch || moving}
          aria-haspopup="menu"
          aria-expanded={branchOpen}
          use:tooltip={streaming ? "Stop the run to switch branch" : "Switch git branch"}
          on:click={() => {
            if (!canSwitchBranch || moving) return;
            branchOpen = !branchOpen;
          }}
        >
          <Icon name="branch" size={12} />
          <span class="info-v mono">{branch || "No branch"}</span>
          {#if canSwitchBranch}<span class="row-chev" aria-hidden="true"><Icon name="chevDown" size={12} /></span>{/if}
        </button>
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
  {#if branchOpen && folder}
    <BranchMenu
      anchor={branchBtn}
      {folder}
      {branch}
      {sessionTitle}
      on:close={() => (branchOpen = false)}
      on:error={(e) => toast(e.detail.text, true)}
      on:switchBranch={(e) => void switchBranch(e.detail.branch)}
      on:moveFolder={(e) => void move(e.detail.path)}
      on:pickFolder={() => void pickFolder()}
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

  <RunningCard {session} {helpers} {notices} {running} {waiting} on:openSession />
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
  .row-chev {
    flex: none;
    display: inline-flex;
    color: var(--faint);
    transition: color var(--dur) ease;
  }
  button.info-line.row:hover .row-chev {
    color: var(--text);
  }
</style>
