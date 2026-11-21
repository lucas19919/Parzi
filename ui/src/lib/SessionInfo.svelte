<script lang="ts">
  import { tokenCount } from "./format";
  import { createEventDispatcher, onMount } from "svelte";
  import Icon from "./Icon.svelte";
  import InfoCard from "./InfoCard.svelte";
  import ContextBreakdown from "./ContextBreakdown.svelte";
  import RunningCard from "./RunningCard.svelte";
  import { board, reloadBoard } from "./providerStore";
  import ResizeGrip from "./ResizeGrip.svelte";
  import { nameOf } from "./providerRows";
  import { onRunEvent, type Question, type SessionMeta } from "./api";
  import type { Approval } from "./live";
  import { folderName } from "./tabs";

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
  const DEFAULT_ORDER = ["env", "context", "usage", "running"];
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

  const dispatch = createEventDispatcher<{
    openSession: { id: string };
    compact: void;
    resize: { width: number };
  }>();

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
</style>
