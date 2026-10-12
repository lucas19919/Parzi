<script lang="ts">
  // One line above the thread when the session is part of a goal: which
  // goal, which step, how far the plan is, and the way to its page.
  import { createEventDispatcher } from "svelte";
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import Icon from "./Icon.svelte";
  import StepRail from "./StepRail.svelte";
  import { statusWord, stepProgress, stepTone, stepWord, toneOf, type SessionPlace } from "./workModel";

  export let place: SessionPlace;

  const dispatch = createEventDispatcher<{ open: { id: string } }>();

  const reduced = typeof matchMedia !== "undefined" && matchMedia("(prefers-reduced-motion: reduce)").matches;

  $: goal = place.goal;
  // A step's session shows how its step stands; the lead, the goal.
  $: tone = !place.root && place.step ? stepTone(place.step) : toneOf(goal);
  $: word = !place.root && place.step ? stepWord(place.step) : statusWord(goal);
  $: progress = stepProgress(goal.steps);
  $: where = place.root
    ? place.index
      ? `step ${place.index} of ${place.total}`
      : progress.total
        ? `${progress.done} of ${progress.total} steps done`
        : ""
    : (place.step?.title ?? "");
</script>

<div class="strip" class:child={!place.root} transition:slide={{ duration: reduced ? 0 : 180, easing: cubicOut }}>
  <Icon name="goal" size={12} />
  <span class="lead">{place.root ? "Goal" : place.step ? `Step ${place.step.key} of` : "Helping with"}</span>
  <span class="title">{goal.title}</span>
  {#if where}<span class="where">{where}</span>{/if}
  {#if goal.steps.length}<span class="rail"><StepRail steps={goal.steps} /></span>{/if}
  <span class="status {tone}">{word}</span>
  <button class="open" on:click={() => dispatch("open", { id: goal.id })}>Open</button>
</div>

<style>
  .strip {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 34px;
    padding: 0 14px 0 20px;
    border-bottom: 1px solid var(--line);
    background: color-mix(in srgb, var(--text) 3%, transparent);
    color: var(--muted);
    font-size: var(--fs-sm);
    white-space: nowrap;
    overflow: hidden;
  }
  .lead {
    flex: none;
  }
  .title {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text);
    font-weight: 500;
  }
  .where {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .where::before,
  .rail::before {
    content: "·";
    margin-right: 8px;
    color: var(--faint);
  }
  .rail {
    flex: none;
    display: inline-flex;
    align-items: center;
  }
  .status {
    flex: none;
    margin-left: auto;
    padding: 1px 8px;
    border-radius: 999px;
    background: var(--hover);
    font-size: var(--fs-xs);
    font-weight: 600;
  }
  .status.live {
    background: color-mix(in srgb, var(--ok) 14%, transparent);
    color: var(--text);
  }
  .status.need {
    background: color-mix(in srgb, var(--warn) 16%, transparent);
    color: var(--text);
  }
  .status.bad {
    background: color-mix(in srgb, var(--bad) 18%, transparent);
    color: var(--text);
  }
  .open {
    flex: none;
    height: 24px;
    padding: 0 10px;
    background: none;
    border: 1px solid var(--line);
    border-radius: var(--radius);
    color: var(--muted);
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .open:hover {
    background: var(--hover);
    color: var(--text);
  }
</style>
