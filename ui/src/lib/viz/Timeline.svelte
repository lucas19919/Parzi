<script lang="ts">
  // Events in order on a hairline rail: a dot per event, when on the right.
  import { whenText, type TimelineItem } from "./spec";

  export let items: TimelineItem[] = [];
  export let more = 0;
  export let label = "";
</script>

<ol class="timeline" aria-label={label || undefined}>
  {#each items as it}
    <li>
      <span class="rail">
        {#if it.status}
          <span class="dot {it.tone}" role="img" aria-label={it.status} />
        {:else}
          <span class="dot {it.tone}" aria-hidden="true" />
        {/if}
      </span>
      <div class="body">
        <div class="head">
          <span class="label">{it.label}</span>
          {#if it.when}<time class="when" datetime={/^\d{4}-\d{2}-\d{2}/.test(it.when) ? it.when : undefined}>{whenText(it.when)}</time>{/if}
        </div>
        {#if it.detail}<p class="detail">{it.detail}</p>{/if}
      </div>
    </li>
  {/each}
</ol>
{#if more > 0}<p class="more">{more} more not shown</p>{/if}

<style>
  .timeline {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    position: relative;
    display: flex;
    gap: calc(var(--space) * 3);
    min-width: 0;
    padding-bottom: calc(var(--space) * 3);
  }
  li:last-child {
    padding-bottom: 0;
  }
  /* The rail: a hairline from this dot down to the next. */
  .rail {
    position: relative;
    display: flex;
    justify-content: center;
    width: 6px;
    flex: none;
    padding-top: calc(var(--space) * 2);
  }
  li:not(:last-child) .rail::after {
    content: "";
    position: absolute;
    top: 17px;
    bottom: -4px;
    left: 2.5px;
    width: 1px;
    background: var(--faint);
  }
  .dot {
    width: 6px;
    height: 6px;
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
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--space);
    flex: 1;
    min-width: 0;
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: calc(var(--space) * 3);
    min-width: 0;
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: var(--fs-md);
    color: var(--text);
  }
  .when {
    flex: none;
    font-size: var(--fs-sm);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .detail {
    margin: 0;
    font-size: var(--fs-sm);
    line-height: 1.5;
    color: var(--muted);
  }
  .more {
    margin: var(--space) 0 0;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
</style>
