<script lang="ts">
  // One goal at a glance: what it is, where it stands, what its agents are
  // doing now and what it cost. Click to open its page.
  import { createEventDispatcher } from "svelte";
  import Icon from "./Icon.svelte";
  import StepRail from "./StepRail.svelte";
  import type { GoalView } from "./api";
  import { prettyModel } from "./providerRows";
  import { activity, now } from "./workStore";
  import { cardLine, engineLabel, formatCost, statusWord, stepProgress, timeOf, toneOf } from "./workModel";

  export let goal: GoalView;
  export let selected = false;
  export let showEngine = false;
  // The dock's narrow cards: no cost line.
  export let compact = false;

  const dispatch = createEventDispatcher<{ open: { id: string } }>();

  $: tone = toneOf(goal);
  $: line = cardLine(goal, $activity);
  $: progress = stepProgress(goal.steps);
  $: model = goal.last?.model ? prettyModel(goal.last.model) : "";
  $: meta = [model, formatCost(goal.cost_usd), goal.files ? `${goal.files} file${goal.files === 1 ? "" : "s"}` : ""].filter(Boolean);
</script>

<button class="goal-card" class:on={selected} class:compact data-goal={goal.id} aria-current={selected ? "true" : undefined} on:click={() => dispatch("open", { id: goal.id })}>
  <span class="top">
    <span class="status {tone}"><i class="sdot" />{statusWord(goal)}</span>
    {#if showEngine && goal.engine.kind !== "here"}
      <span class="engine"><Icon name={goal.engine.kind === "server" ? "server" : "desktop"} size={12} />{engineLabel(goal.engine)}</span>
    {/if}
    <span class="time">{timeOf(goal, $now)}</span>
  </span>
  <span class="title">{goal.title}</span>
  {#if goal.steps.length}
    <span class="plan">
      <StepRail steps={goal.steps} />
      <span class="count">{progress.done} of {progress.total}</span>
    </span>
  {/if}
  {#if line.text}
    <span class="line" class:live={line.live}>
      {#if line.live}<i class="pulse" />{/if}
      <span class="text">{line.text}</span>
      {#if line.more > 0}<span class="also">+{line.more} more</span>{/if}
    </span>
  {/if}
  {#if !compact && meta.length}
    <span class="meta">{meta.join(" · ")}</span>
  {/if}
</button>

<style>
  .goal-card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
    min-width: 0;
    padding: 10px 12px 11px;
    background: color-mix(in srgb, var(--text) 3%, transparent);
    border: 1px solid var(--line);
    border-radius: var(--radius-lg);
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .goal-card:hover {
    background: var(--hover);
  }
  .goal-card.on {
    background: var(--selected);
    border-color: color-mix(in srgb, var(--accent) 45%, var(--line));
  }
  .compact {
    gap: 4px;
    padding: 8px 10px 9px;
  }
  .top {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    font-size: var(--fs-xs);
  }
  .status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    font-weight: 600;
    color: var(--muted);
    white-space: nowrap;
  }
  .sdot {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--faint);
  }
  .status.live .sdot {
    background: var(--ok);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--ok) 22%, transparent);
  }
  .status.need .sdot {
    background: var(--warn);
  }
  .status.bad .sdot {
    background: var(--bad);
  }
  .status.ok .sdot {
    background: var(--muted);
  }
  .status.live,
  .status.need,
  .status.bad {
    color: var(--text);
  }
  .engine {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--hover);
    color: var(--muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .time {
    flex: none;
    margin-left: auto;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .title {
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
    font-size: var(--fs-base);
    font-weight: 600;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }
  .compact .title {
    font-size: var(--fs-md);
  }
  .plan {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .count {
    font-size: var(--fs-xs);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }
  .line {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .line.live {
    color: var(--text);
  }
  .text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .also {
    flex: none;
    color: var(--muted);
  }
  .pulse {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--ok);
    animation: pulse 1.6s var(--ease-out) infinite;
  }
  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }
  .meta {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-xs);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
</style>
