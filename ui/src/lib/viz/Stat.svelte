<script lang="ts">
  // One number with its label, how it moved, and its recent trend.
  import Spark from "./Spark.svelte";
  import type { StatItem } from "./spec";

  export let stat: StatItem;
  export let unit = "";
</script>

<div class="stat">
  {#if stat.label}<span class="label">{stat.label}</span>{/if}
  <div class="figure">
    <span class="value">{stat.value}</span>
    {#if stat.delta}<span class="delta">{stat.delta}</span>{/if}
  </div>
  {#if stat.spark.length}
    <Spark values={stat.spark} height={28} {unit} label={stat.label} />
  {/if}
</div>

<style>
  .stat {
    display: flex;
    flex-direction: column;
    gap: var(--space);
    min-width: 0;
  }
  .label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .figure {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: var(--space) calc(var(--space) * 2);
    min-width: 0;
  }
  /* Proportional digits: a lone big number reads tighter than tabular. */
  .value {
    font-size: var(--fs-2xl);
    font-weight: 600;
    line-height: 1.2;
    letter-spacing: -0.01em;
    color: var(--text);
  }
  .delta {
    font-size: var(--fs-sm);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
</style>
