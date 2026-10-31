<script lang="ts">
  import { onDestroy } from "svelte";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import { mdHtml, richReady } from "./md";
  import type { Step } from "./live";

  export let reasoning = "";
  export let tools: Step[] = [];
  export let title = "";
  export let status = "";

  let open = false;
  let shown = new Set<string>();

  $: failed = tools.filter((t) => !t.running && t.ok === false).length;
  $: total = tools.reduce((sum, t) => sum + (t.ms ?? 0), 0);
  $: summary = title || describe(reasoning, tools, total);
  $: expandable = !!reasoning || tools.length > 0;

  function duration(ms: number) {
    if (ms >= 60_000) return `${Math.floor(ms / 60_000)}m ${Math.round((ms % 60_000) / 1000)}s`;
    if (ms >= 1000) return `${(ms / 1000).toFixed(1)}s`;
    return `${Math.round(ms)}ms`;
  }

  let elapsed = 0;
  let timer: number | null = null;
  $: live = !!status;
  $: {
    if (live && timer === null) {
      const t0 = Date.now();
      elapsed = 0;
      timer = window.setInterval(() => {
        elapsed = Date.now() - t0;
      }, 1000);
    } else if (!live && timer !== null) {
      window.clearInterval(timer);
      timer = null;
      elapsed = 0;
    }
  }
  onDestroy(() => {
    if (timer !== null) window.clearInterval(timer);
  });
  function clock(ms: number): string {
    const s = Math.floor(ms / 1000);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }

  function describe(reasoning: string, tools: Step[], total: number) {
    if (!reasoning && tools.length === 1) return tools[0].label.trim() || "Working";
    const parts: string[] = [];
    if (reasoning) parts.push("Thought");
    if (tools.length) parts.push(`${tools.length} tool${tools.length === 1 ? "" : "s"}`);
    if (total >= 1000) parts.push(duration(total));
    return parts.join(" · ") || "Working";
  }

  function toggle(id: string) {
    shown = new Set(shown);
    if (shown.has(id)) shown.delete(id);
    else shown.add(id);
  }
</script>

<div class="steps">
  <button class="head" class:live={!!status} disabled={!expandable} aria-expanded={open} on:click={() => (open = !open)}>
    <span class="text">{status || summary}{#if live && elapsed >= 10_000} · {clock(elapsed)}{/if}</span>
    {#if failed && !status}<span class="failed">{failed} failed</span>{/if}
    {#if expandable}<span class="chev" class:open><Icon name="chevDown" size={12} /></span>{/if}
  </button>

  {#if open}
    <div class="body" data-ui-list transition:slide={{ duration: 160, easing: cubicOut }}>
      {#if reasoning}
        <div class="reasoning">{reasoning}</div>
      {/if}
      {#each tools as t (t.id)}
        <button class="tool" disabled={!t.output} on:click={() => toggle(t.id)}>
          <span class="label">{t.label}</span>
          {#if t.running}
            <span class="meta live">Running</span>
          {:else if t.ok === false}
            <span class="meta bad">Failed</span>
          {:else if t.ms}
            <span class="meta">{duration(t.ms)}</span>
          {/if}
        </button>
        {#if t.output && shown.has(t.id)}
          <div class="output">{@html mdHtml("```text\n" + t.output.slice(0, 6000) + "\n```", $richReady)}</div>
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  .steps {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .head {
    align-self: flex-start;
    display: inline-flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    max-width: 100%;
    min-height: var(--h-sm);
    padding: 0;
    background: none;
    border: none;
    color: var(--muted);
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .head:hover:not(:disabled) {
    color: var(--text);
  }
  .head:disabled {
    cursor: default;
  }
  .head.live .text {
    animation: breathe 1.6s ease-in-out infinite;
  }
  .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .failed {
    color: var(--bad);
  }
  .chev {
    display: inline-flex;
    flex: none;
    transform: rotate(-90deg);
    transition: transform var(--dur) ease;
  }
  .chev.open {
    transform: none;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space);
    margin: calc(var(--space) * 2) 0 var(--space) var(--space);
    padding-left: calc(var(--space) * 3);
    border-left: 1px solid var(--line);
  }
  .reasoning {
    max-height: 320px;
    overflow-y: auto;
    padding: var(--space) 0 calc(var(--space) * 2);
    color: var(--muted);
    font-size: var(--fs-md);
    line-height: 1.6;
    white-space: pre-wrap;
    word-break: break-word;
  }
  .tool {
    display: flex;
    align-items: baseline;
    gap: calc(var(--space) * 3);
    width: 100%;
    padding: var(--space) 0;
    background: none;
    border: none;
    color: var(--muted);
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .tool:disabled {
    cursor: default;
  }
  .tool:hover:not(:disabled) .label {
    color: var(--text);
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .meta {
    flex: none;
    font-family: var(--mono);
    font-size: var(--fs-xs);
    color: var(--muted);
  }
  .meta.bad {
    color: var(--bad);
  }
  .meta.live {
    animation: breathe 1.6s ease-in-out infinite;
  }
  .output {
    font-size: var(--fs-sm);
  }
  .output :global(.codeblock) {
    margin: var(--space) 0 calc(var(--space) * 2);
  }
  @keyframes breathe {
    50% {
      opacity: 0.45;
    }
  }
</style>
