<script lang="ts">
  // A goal's plan at a glance. Compact: a row of dots (cards, the session
  // strip). Vertical: one row per step with its needs drawn as lines, and a
  // slot for what the goal page says about the step.
  import type { StepView } from "./api";
  import { compactRail, railLayout, stepProgress, type RailLine } from "./workModel";

  export let steps: StepView[] = [];
  export let variant: "compact" | "vertical" = "compact";

  // Vertical geometry: column pitch, left inset, the dot's centre from the
  // row top (level with the first line of text), and how far below the dot a
  // branching line takes to reach its column.
  const COL = 14;
  const PAD = 8;
  const DOT_Y = 15;
  const BEND = 14;
  const x = (c: number) => PAD + c * COL;

  $: rail = compactRail(steps);
  $: progress = stepProgress(steps);
  $: label = `${progress.done} of ${progress.total} steps done`;
  $: layout = variant === "vertical" ? railLayout(steps) : { rows: [], cols: 1 };
  $: graphW = PAD * 2 + (layout.cols - 1) * COL;

  function into(l: RailLine, col: number): string {
    const x1 = x(l.col);
    const x2 = x(col);
    return `M ${x1} 0 C ${x1} ${DOT_Y * 0.6} ${x2} ${DOT_Y * 0.4} ${x2} ${DOT_Y}`;
  }

  function out(l: RailLine, col: number): string {
    const x1 = x(col);
    const x2 = x(l.col);
    return `M ${x1} ${DOT_Y} C ${x1} ${DOT_Y + BEND * 0.6} ${x2} ${DOT_Y + BEND * 0.4} ${x2} ${DOT_Y + BEND}`;
  }
</script>

{#if variant === "compact"}
  <span class="rail" role="img" aria-label={label}>
    {#each rail.dots as d (d.key)}
      {#if d.next === "line"}<i class="join" />{:else if d.next === "side"}<i class="side" />{/if}
      <i class="dot {d.status}" />
    {/each}
    {#if rail.more}<span class="more">+{rail.more}</span>{/if}
  </span>
{:else}
  <ol class="vrail" aria-label={label}>
    {#each layout.rows as row (row.step.key)}
      <li class="vrow" style:padding-left="{graphW + 6}px">
        <svg class="graph" width={graphW} aria-hidden="true">
          {#each row.through as l}
            <line class="ln" class:met={l.met} x1={x(l.col)} y1="0" x2={x(l.col)} y2="100%" />
          {/each}
          {#each row.into as l}
            <path class="ln" class:met={l.met} d={into(l, row.col)} />
          {/each}
          {#each row.out as l}
            {#if l.col === row.col}
              <line class="ln" class:met={l.met} x1={x(l.col)} y1={DOT_Y} x2={x(l.col)} y2="100%" />
            {:else}
              <path class="ln" class:met={l.met} d={out(l, row.col)} />
              <line class="ln" class:met={l.met} x1={x(l.col)} y1={DOT_Y + BEND} x2={x(l.col)} y2="100%" />
            {/if}
          {/each}
          {#if row.step.status === "running"}<circle class="halo" cx={x(row.col)} cy={DOT_Y} r="8" />{/if}
          <circle class="node {row.step.status}" class:waiting={row.step.status === "todo" && row.step.blocked_by.length > 0} cx={x(row.col)} cy={DOT_Y} r="5" />
        </svg>
        <slot {row} />
      </li>
    {/each}
  </ol>
{/if}

<style>
  .rail {
    display: inline-flex;
    align-items: center;
    min-width: 0;
    height: 12px;
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: 50%;
    box-sizing: border-box;
    border: 1.5px solid var(--faint);
  }
  .dot.done {
    background: var(--muted);
    border-color: var(--muted);
  }
  .dot.running {
    background: var(--ok);
    border-color: var(--ok);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--ok) 28%, transparent);
    animation: beat 1.6s var(--ease-out) infinite;
  }
  .dot.failed {
    background: var(--bad);
    border-color: var(--bad);
  }
  .dot.check {
    background: var(--warn);
    border-color: var(--warn);
  }
  .dot.skipped {
    opacity: 0.5;
  }
  .join {
    flex: none;
    width: 8px;
    height: 1.5px;
    margin: 0 2px;
    border-radius: 1px;
    background: var(--faint);
  }
  .side {
    flex: none;
    width: 3px;
  }
  .more {
    margin-left: 6px;
    font-size: var(--fs-xs);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  @keyframes beat {
    50% {
      box-shadow: 0 0 0 4px color-mix(in srgb, var(--ok) 0%, transparent);
    }
  }

  .vrail {
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .vrow {
    position: relative;
    min-height: 44px;
  }
  .graph {
    position: absolute;
    top: 0;
    left: 0;
    height: 100%;
    overflow: visible;
  }
  .ln {
    fill: none;
    stroke: var(--faint);
    stroke-width: 1.5;
    stroke-dasharray: 3 3;
  }
  .ln.met {
    stroke: color-mix(in srgb, var(--muted) 70%, transparent);
    stroke-dasharray: none;
  }
  .node {
    fill: var(--bg);
    stroke: var(--faint);
    stroke-width: 1.5;
  }
  .node.waiting {
    stroke-dasharray: 2 2;
  }
  .node.done {
    fill: var(--muted);
    stroke: var(--muted);
  }
  .node.running {
    fill: var(--ok);
    stroke: var(--ok);
  }
  .node.failed {
    fill: var(--bad);
    stroke: var(--bad);
  }
  .node.check {
    fill: var(--warn);
    stroke: var(--warn);
  }
  .node.skipped {
    opacity: 0.5;
  }
  .halo {
    fill: color-mix(in srgb, var(--ok) 22%, transparent);
    animation: halo 1.6s var(--ease-out) infinite;
    transform-box: fill-box;
    transform-origin: center;
  }
  @keyframes halo {
    50% {
      opacity: 0.25;
      transform: scale(1.2);
    }
  }
</style>
