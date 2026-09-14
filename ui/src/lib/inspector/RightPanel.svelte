<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import DocReader from "./DocReader.svelte";
  import AgentVisualizer from "./AgentVisualizer.svelte";
  import type { ChatEvent, DocEntry, InspectorArtifact, InspectorDoc, SwarmNode } from "../api";

  export let tab: "docs" | "agents" = "docs";
  export let width = 420;
  export let autoReveal = true;

  // Docs tab
  export let artifact: InspectorArtifact | null = null;
  export let artifacts: InspectorArtifact[] = [];
  export let doc: InspectorDoc | null = null;
  export let docs: DocEntry[] = [];
  export let docLoading = false;

  // Agents tab
  export let nodes: SwarmNode[] = [];
  export let activeThreadId: string | null = null;
  export let events: ChatEvent[] = [];
  export let approval: { key: string; call: { id: string; name: string; args: unknown; lane: string } } | null = null;
  export let logos: Record<string, string> = {};

  export const MIN_W = 340;
  export const MAX_W = 680;

  const dispatch = createEventDispatcher<{
    close: void;
    tab: { tab: "docs" | "agents" };
    resize: { width: number };
    autoReveal: { on: boolean };
    openDoc: { entry: DocEntry };
    openTranscript: void;
    openArtifact: { artifact: InspectorArtifact };
    pickFile: void;
    focus: { id: string };
    fork: { id: string };
    kill: { id: string };
    spawn: { id: string };
    approve: { key: string; allow: boolean };
  }>();

  $: liveAgents = nodes.filter((n) => n.status === "active" || n.status === "queued").length;

  /* ---------- drag-to-resize on the left edge ---------- */
  let dragging = false;
  let startX = 0;
  let startW = 0;
  function onGripDown(e: PointerEvent) {
    if (e.button !== 0) return;
    dragging = true;
    startX = e.clientX;
    startW = width;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    e.preventDefault();
  }
  function onGripMove(e: PointerEvent) {
    if (!dragging) return;
    const w = Math.round(Math.min(MAX_W, Math.max(MIN_W, startW - (e.clientX - startX))));
    if (w !== width) dispatch("resize", { width: w });
  }
  function onGripUp(e: PointerEvent) {
    if (!dragging) return;
    dragging = false;
    try { (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId); } catch {}
  }
  function onGripKey(e: KeyboardEvent) {
    const step = e.shiftKey ? 40 : 12;
    if (e.key === "ArrowLeft") dispatch("resize", { width: Math.min(MAX_W, width + step) });
    else if (e.key === "ArrowRight") dispatch("resize", { width: Math.max(MIN_W, width - step) });
    else return;
    e.preventDefault();
  }
</script>

<aside class="deck" class:dragging aria-label="Inspector deck">
  <div class="grip" role="slider" aria-orientation="vertical" aria-label="Inspector width" tabindex="0"
    aria-valuemin={MIN_W} aria-valuemax={MAX_W} aria-valuenow={width}
    on:pointerdown={onGripDown} on:pointermove={onGripMove} on:pointerup={onGripUp} on:pointercancel={onGripUp} on:keydown={onGripKey} />

  <header class="head">
    <div class="tabs" role="tablist">
      <button class="tab" class:on={tab === "docs"} role="tab" aria-selected={tab === "docs"} on:click={() => tab === "docs" ? dispatch("close") : dispatch("tab", { tab: "docs" })}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M14 3H6a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V9z" /><path d="M14 3v6h6" /></svg>
        Docs
        {#if artifact || doc}<span class="tab-dot" />{/if}
      </button>
      <button class="tab" class:on={tab === "agents"} role="tab" aria-selected={tab === "agents"} on:click={() => tab === "agents" ? dispatch("close") : dispatch("tab", { tab: "agents" })}>
        <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M13 2L4 14h7l-1 8 9-12h-7l1-8z" /></svg>
        Agents
        {#if nodes.length}<span class="count" class:live={liveAgents > 0}>{liveAgents || nodes.length}</span>{/if}
      </button>
    </div>
    <span class="spacer" />
    <button class="hb" class:on={autoReveal} title={autoReveal ? "Auto-open on artifacts and spawns: on" : "Auto-open on artifacts and spawns: off"}
      aria-pressed={autoReveal} on:click={() => dispatch("autoReveal", { on: !autoReveal })}>
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M2 12s3.5-7 10-7 10 7 10 7-3.5 7-10 7-10-7-10-7z" /><circle cx="12" cy="12" r="3" /></svg>
    </button>
    <button class="hb" title="Close inspector (Ctrl+\)" on:click={() => dispatch("close")}>
      <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6L6 18M6 6l12 12" /></svg>
    </button>
  </header>

  <div class="content">
    {#if tab === "docs"}
      <DocReader
        {artifact} {artifacts} {doc} {docs} loading={docLoading} hasThread={!!activeThreadId}
        on:openDoc on:openTranscript on:openArtifact on:pickFile
      />
    {:else}
      <AgentVisualizer {nodes} {activeThreadId} {events} {approval} {logos}
        on:focus on:fork on:kill on:spawn on:approve />
    {/if}
  </div>
</aside>

<style>
  .deck {
    position: relative; height: 100%; width: 100%; display: flex; flex-direction: column; min-height: 0;
    background: var(--panel, rgba(26, 29, 36, 0.85));
    border-left: 1px solid var(--line, rgba(255,255,255,0.1));
    backdrop-filter: blur(var(--glass-blur, 18px)) saturate(1.3);
    -webkit-backdrop-filter: blur(var(--glass-blur, 18px)) saturate(1.3);
    box-shadow: inset 1px 0 0 var(--line-hi, rgba(255,255,255,0.06));
  }
  .deck.dragging { user-select: none; }
  .deck.dragging .content { pointer-events: none; }
  .grip {
    position: absolute; left: -4px; top: 0; bottom: 0; width: 8px; cursor: col-resize; z-index: 3;
    touch-action: none;
  }
  .grip::after {
    content: ""; position: absolute; left: 3px; top: 0; bottom: 0; width: 2px; border-radius: 2px;
    background: transparent; transition: background 0.12s ease;
  }
  .grip:hover::after, .deck.dragging .grip::after, .grip:focus-visible::after { background: var(--accent-line, rgba(124,140,255,0.4)); }
  .grip:focus-visible { outline: none; }

  .head {
    display: flex; align-items: center; gap: 4px; height: 38px; flex: none; padding: 0 8px 0 10px;
    border-bottom: 1px solid var(--line-2, rgba(255,255,255,0.07));
  }
  .tabs { display: inline-flex; gap: 2px; }
  .tab {
    display: inline-flex; align-items: center; gap: 6px; height: 26px; padding: 0 10px;
    background: transparent; border: 1px solid transparent; border-radius: var(--radius-2, 8px);
    color: var(--text-3, #94a3b8); font: inherit; font-size: 12px; font-weight: 500; cursor: pointer;
  }
  .tab:hover { color: var(--text, #fff); background: var(--surface-1, rgba(255,255,255,0.05)); }
  .tab.on { color: var(--text, #fff); background: var(--surface-2, rgba(255,255,255,0.07)); border-color: var(--line-2, rgba(255,255,255,0.08)); }
  .tab-dot { width: 5px; height: 5px; border-radius: 50%; background: var(--accent, #7c8cff); }
  .count {
    font-family: var(--parzi-mono, monospace); font-size: 10px; min-width: 16px; height: 16px; padding: 0 5px;
    display: inline-flex; align-items: center; justify-content: center; border-radius: 999px;
    background: var(--surface-3, rgba(255,255,255,0.1)); color: var(--text-3, #94a3b8);
  }
  .count.live { background: var(--ok-soft, rgba(74,222,128,0.14)); color: var(--ok, #4ade80); }
  .spacer { flex: 1; }
  .hb {
    width: 26px; height: 26px; display: inline-flex; align-items: center; justify-content: center;
    background: transparent; border: 1px solid transparent; border-radius: 7px;
    color: var(--text-3, #94a3b8); cursor: pointer; padding: 0;
  }
  .hb:hover { background: var(--surface-2, rgba(255,255,255,0.07)); color: var(--text, #fff); }
  .hb.on { color: var(--accent, #7c8cff); }
  .content { flex: 1; min-height: 0; display: flex; flex-direction: column; }
</style>
