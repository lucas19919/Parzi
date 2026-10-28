<script lang="ts">
  // Meters: how far each thing got, like Home's agent usage rows.
  import type { ProgressItem } from "./spec";

  export let items: ProgressItem[] = [];

  $: labelled = items.some((p) => p.label);
</script>

<div class="meters" class:labelled>
  {#each items as p}
    <div class="meter">
      {#if labelled}<span class="label">{p.label}</span>{/if}
      <span
        class="track"
        role="progressbar"
        aria-label={p.label || "Progress"}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={Math.round(p.value * 100)}
        aria-valuetext={p.text}
      >
        <i style:width="{(p.value * 100).toFixed(1)}%" />
      </span>
      <span class="value">{p.text}</span>
    </div>
  {/each}
</div>

<style>
  .meters {
    display: grid;
    grid-template-columns: minmax(40px, 1fr) max-content;
    column-gap: calc(var(--space) * 3);
  }
  .meters.labelled {
    grid-template-columns: fit-content(40%) minmax(40px, 1fr) max-content;
  }
  .meter {
    display: grid;
    grid-column: 1 / -1;
    grid-template-columns: subgrid;
    align-items: center;
    height: 26px;
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
    color: var(--text);
  }
  .track {
    display: block;
    height: 6px;
    overflow: hidden;
    border-radius: 999px;
    background: var(--selected);
  }
  .track i {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
    transition: width 400ms var(--ease-out);
  }
  .value {
    font-size: var(--fs-sm);
    color: var(--muted);
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
</style>
