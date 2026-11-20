<script lang="ts">
  import { tokenCount } from "./format";
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import ContextBreakdown from "./ContextBreakdown.svelte";
  import HostsCard from "./HostsCard.svelte";
  import RunningCard from "./RunningCard.svelte";
  import FilesCard from "./FilesCard.svelte";
  import type { ArtifactDoc } from "./tabs";
  import { board, reloadBoard } from "./providerStore";
  import ResizeGrip from "./ResizeGrip.svelte";
  import { nameOf } from "./providerRows";
  import { api, onRunEvent, type GitStatus, type PrInfo, type Question, type SessionMeta } from "./api";
  import type { Approval } from "./live";
  import { isTrunk } from "./sessionTone";
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
  export let width = 264;
  // Card order: environment first. Swappable by drag,
  // persisted per machine.
  const ORDER_KEY = "parzi.info.order";
  const DEFAULT_ORDER = ["env", "context", "usage", "changes", "files", "hosts", "running"];
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
  /** A card's slot: drag it onto another to swap their places. */
  function movable(node: HTMLElement, id: string) {
    node.draggable = true;
    const start = (e: DragEvent) => {
      dragId = id;
      if (e.dataTransfer) {
        e.dataTransfer.effectAllowed = "move";
        try {
          e.dataTransfer.setData("text/plain", id);
        } catch {}
      }
    };
    const over = (e: DragEvent) => e.preventDefault();
    const drop = (e: DragEvent) => onDrop(id, e);
    node.addEventListener("dragstart", start);
    node.addEventListener("dragover", over);
    node.addEventListener("drop", drop);
    return {
      update(next: string) {
        id = next;
      },
      destroy() {
        node.removeEventListener("dragstart", start);
        node.removeEventListener("dragover", over);
        node.removeEventListener("drop", drop);
      },
    };
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
    resize: { width: number };
    openFile: ArtifactDoc;
  }>();

  $: pct = contextLimit > 0 ? Math.min(100, Math.round((contextUsed / contextLimit) * 100)) : 0;

  const gitCache = new Map<string, { git: GitStatus | null; prs: PrInfo[] }>();
  let git: GitStatus | null = null;
  let gitFor: string | null = null;
  $: gitDir = remoteHost ? "" : folder;
  $: if (gitDir !== gitFor) {
    gitFor = gitDir;
    const cached = gitDir ? gitCache.get(gitDir) : null;
    git = cached?.git ?? null;
    prs = cached?.prs ?? [];
    void loadGit();
  }
  async function loadGit() {
    const dir = gitFor;
    const s = dir ? await api.gitStatus(dir).catch(() => null) : null;
    if (dir === gitFor) {
      git = s;
      if (dir) gitCache.set(dir, { git: s, prs });
    }
    // gh is slow and cached a minute in the backend: after the status.
    const found = dir ? await api.branchPrs(dir).catch(() => []) : [];
    if (dir === gitFor) {
      prs = found;
      if (dir) gitCache.set(dir, { git, prs: found });
    }
  }
  // The Changes card: one row per changed file, a click shows its diff.
  const CHANGE_ROWS = 8;
  let allChanges = false;
  type ChangeRow = { path: string; abs: string; name: string; dir: string; added: number; removed: number; fresh: boolean };
  $: changeRows = ((): ChangeRow[] => {
    if (!git?.files?.length) return [];
    const stat = new Map((git.stats ?? []).map((s) => [s.path, s]));
    const top = (git.top || folder).replace(/[\\/]+$/, "");
    const sep = top.includes("\\") ? "\\" : "/";
    return git.files.map((path) => {
      const s = stat.get(path);
      const cut = path.lastIndexOf("/");
      return {
        path,
        abs: `${top}${sep}${path.replace(/\//g, sep)}`,
        name: path.slice(cut + 1),
        dir: cut > 0 ? path.slice(0, cut) : "",
        added: s?.added ?? 0,
        removed: s?.removed ?? 0,
        fresh: !s,
      };
    });
  })();
  async function showDiff(f: ChangeRow) {
    const content = await api.gitDiff(folder, f.path).catch((e) => `Couldn't read the diff: ${e}`);
    dispatch("openFile", { id: f.abs, title: f.name, kind: "diff", content: content || "No changes against HEAD.", version: Date.now() });
  }
  let prs: PrInfo[] = [];
  $: pr = branch && !isTrunk(branch) ? (prs.find((p) => p.head === branch && p.state === "open") ?? null) : null;

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
  <div class="slot" style="order: {pos('env')}" use:movable={"env"}>
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

  {#if contextLimit > 0}
  <div class="slot" style="order: {pos('context')}" use:movable={"context"}>
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
  <div class="slot" style="order: {pos('usage')}" use:movable={"usage"}>
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

  {#if git && (changeRows.length || pr || (git.upstream && (git.ahead || git.behind)))}
  <div class="slot" style="order: {pos('changes')}" use:movable={"changes"}>
    <InfoCard id="changes" title="Changes">
      <svelte:fragment slot="aside">
        {#if git.added || git.removed}
          <span class="add">+{git.added.toLocaleString()}</span>&nbsp;<span class="del">−{git.removed.toLocaleString()}</span>
        {/if}
      </svelte:fragment>
      {#each changeRows.slice(0, allChanges ? changeRows.length : CHANGE_ROWS) as f (f.path)}
        <button class="info-line row file" class:on={reading === f.abs} use:tooltip={`${f.path}: show the diff`} on:click={() => showDiff(f)}>
          <span class="fname">
            <span class="info-v">{f.name}</span>
            {#if f.dir}<span class="fdir">{f.dir}</span>{/if}
          </span>
          {#if f.fresh}
            <span class="add n">new</span>
          {:else if f.added || f.removed}
            <span class="n">{#if f.added}<span class="add">+{f.added}</span>{/if} {#if f.removed}<span class="del">−{f.removed}</span>{/if}</span>
          {/if}
        </button>
      {/each}
      {#if changeRows.length > CHANGE_ROWS}
        <button class="info-line row quiet" on:click={() => (allChanges = !allChanges)}>
          <span class="info-v">{allChanges ? "Show fewer" : `${changeRows.length - CHANGE_ROWS} more`}</span>
        </button>
      {/if}
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

  <div class="slot" style="order: {pos('files')}" use:movable={"files"}>
  <FilesCard {folder} shown={open} current={reading} on:open={(e) => dispatch("openFile", e.detail)} />
  </div>

  <div class="slot" style="order: {pos('hosts')}" use:movable={"hosts"}>
  <HostsCard {session} on:openUrl on:openFile />
  </div>

  <div class="slot" style="order: {pos('running')}" use:movable={"running"}>
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
  /* A card that has nothing to say renders nothing; its slot takes no gap. */
  .slot:not(:has(*)) {
    display: none;
  }
  /* Every slot is draggable (use:movable). */
  .slot {
    cursor: grab;
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
  .fname {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: var(--space);
    overflow: hidden;
  }
  .fname .info-v {
    flex: none;
    max-width: 100%;
  }
  .fdir {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    direction: rtl;
    text-align: left;
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .n {
    flex: none;
    font-size: var(--fs-xs);
    font-variant-numeric: tabular-nums;
  }
  .file.on {
    background: var(--hover);
  }
  .checks.pass :global(svg) {
    color: var(--ok-text);
  }
  .checks.fail :global(svg) {
    color: var(--bad-text);
  }
</style>
