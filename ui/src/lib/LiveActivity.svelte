<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import ThinkingFace from "./ThinkingFace.svelte";
  import type { LiveTool } from "./live";

  /** What a running agent is doing, in the thread, without opening a fold:
   *  its last few steps as they run, and the last thing it said or thought.
   *  At the end of the turn it folds into "Worked for…" with the rest. */
  export let tools: LiveTool[] = [];
  export let said = "";
  export let shown = 3;
  /** Subagents this session started that are still working (or waiting
   *  on the person), each with the step it is on. */
  export let helpers: { id: string; title: string; need: boolean; step: string }[] = [];

  const dispatch = createEventDispatcher<{ open: { id: string } }>();

  $: recent = tools.slice(-shown);
  const secs = (ms: number) => (ms >= 60_000 ? `${Math.floor(ms / 60_000)}m ${Math.round((ms % 60_000) / 1000)}s` : `${(ms / 1000).toFixed(1)}s`);
</script>

<div class="activity">
  {#if said}<p class="said">{said}</p>{/if}
  {#each recent as t (t.id)}
    <div class="step" class:done={!t.running} class:failed={!t.running && !t.ok} in:fly|local={{ y: 4, duration: 160, easing: cubicOut }}>
      <span class="mark">
        {#if t.running}<span class="spin" aria-label="Running" />{:else if t.ok}<Icon name="check" size={12} />{:else}<Icon name="close" size={12} />{/if}
      </span>
      <span class="label">{t.label}</span>
      {#if !t.running && t.ms >= 1000}<span class="ms">{secs(t.ms)}</span>{/if}
    </div>
  {/each}
  {#if helpers.length}
    <div class="helpers" class:gap={!!recent.length || !!said}>
      {#each helpers as h (h.id)}
        <button class="helper" on:click={() => dispatch("open", { id: h.id })} in:fly|local={{ y: 4, duration: 160, easing: cubicOut }}>
          <span class="mark">{#if h.need}<i class="need" />{:else}<ThinkingFace size={14} />{/if}</span>
          <span class="who">{h.title}</span>
          <span class="what">{h.need ? "needs you" : h.step || "working"}</span>
          <span class="go"><Icon name="arrowRight" size={12} /></span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .activity {
    display: flex;
    flex-direction: column;
    gap: var(--space);
    padding-left: calc(var(--space) * 3);
    border-left: 1px solid var(--line);
  }
  .said {
    margin: 0 0 var(--space);
    color: var(--muted);
    font-size: var(--fs-md);
    line-height: 1.5;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .step {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-width: 0;
    height: 20px;
    font-size: var(--fs-md);
    color: var(--text);
  }
  .step.done {
    color: var(--muted);
  }
  .step.failed .mark {
    color: var(--bad-text);
  }
  .mark {
    flex: none;
    width: 14px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--muted);
  }
  .spin {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    border: 1.5px solid color-mix(in srgb, var(--text) 25%, transparent);
    border-top-color: var(--text);
    animation: spin 0.9s linear infinite;
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ms {
    flex: none;
    margin-left: auto;
    font-size: var(--fs-xs);
    color: var(--faint);
    font-variant-numeric: tabular-nums;
  }
  .helpers {
    display: flex;
    flex-direction: column;
    gap: var(--space);
  }
  .helpers.gap {
    margin-top: var(--space);
  }
  .helper {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 2);
    min-width: 0;
    height: 24px;
    margin-left: calc(var(--space) * -2);
    padding: 0 calc(var(--space) * 2);
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
    text-align: left;
    cursor: pointer;
  }
  .helper:hover {
    background: var(--hover);
  }
  .who {
    flex: none;
    max-width: 45%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .what {
    min-width: 0;
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .go {
    flex: none;
    margin-left: auto;
    display: inline-flex;
    color: var(--faint);
    opacity: 0;
  }
  .helper:hover .go {
    opacity: 1;
  }
  .need {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--warn);
  }
  @media (prefers-reduced-motion: reduce) {
    .spin {
      animation: none;
    }
  }
</style>
