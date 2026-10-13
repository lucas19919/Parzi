<script lang="ts">
  // Rows like Home's: a status dot, the name, a note, a value at the end.
  import type { ListItem } from "./spec";

  export let items: ListItem[] = [];
  export let more = 0;
  export let label = "";
</script>

<ul class="list" aria-label={label || undefined}>
  {#each items as it}
    <li>
      {#if it.status}
        <span class="dot {it.tone}" role="img" aria-label={it.status} />
      {:else}
        <span class="dot {it.tone}" aria-hidden="true" />
      {/if}
      <span class="label">{it.label}</span>
      {#if it.detail}<span class="detail">{it.detail}</span>{/if}
      {#if it.value}<span class="value">{it.value}</span>{/if}
    </li>
  {/each}
</ul>
{#if more > 0}<p class="more">{more} more not shown</p>{/if}

<style>
  .list {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
    height: 32px;
  }
  .dot {
    width: 6px;
    height: 6px;
    flex: none;
    border-radius: 50%;
    background: var(--faint);
  }
  .dot.ok {
    background: var(--ok);
  }
  .dot.warn {
    background: var(--warn);
  }
  .dot.bad {
    background: var(--bad);
  }
  .dot.accent {
    background: var(--accent);
  }
  .label {
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-md);
    color: var(--text);
  }
  .detail {
    flex: 1 1 0;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
  .value {
    flex: none;
    max-width: 45%;
    margin-left: auto;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-sm);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .more {
    margin: 4px 0 0;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
</style>
