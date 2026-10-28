<script lang="ts">
  // Rows and columns: hairlines between rows, numbers right-aligned in
  // tabular figures so they line up.
  export let columns: string[] = [];
  export let rows: string[][] = [];
  export let numeric: boolean[] = [];
  export let more = 0;
  export let label = "";

  $: header = columns.some((c) => c);
</script>

<div class="wrap">
  <table aria-label={label || undefined}>
    {#if header}
      <thead>
        <tr>{#each columns as c, i}<th scope="col" class:num={numeric[i]}>{c}</th>{/each}</tr>
      </thead>
    {/if}
    <tbody>
      {#each rows as r}
        <tr>{#each r as c, i}<td class:num={numeric[i]}>{c}</td>{/each}</tr>
      {/each}
    </tbody>
  </table>
</div>
{#if more > 0}<p class="more">{more} more {more === 1 ? "row" : "rows"} not shown</p>{/if}

<style>
  .wrap {
    max-width: 100%;
    overflow-x: auto;
  }
  table {
    width: 100%;
    border-collapse: collapse;
    font-size: var(--fs-md);
  }
  th,
  td {
    height: 32px;
    padding: 0 calc(var(--space) * 3) 0 0;
    border-bottom: 1px solid var(--line);
    text-align: left;
    white-space: nowrap;
  }
  th:last-child,
  td:last-child {
    padding-right: 0;
  }
  tbody tr:last-child td {
    border-bottom: none;
  }
  th {
    height: 28px;
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--muted);
  }
  td {
    color: var(--text);
  }
  .num {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .more {
    margin: var(--space) 0 0;
    font-size: var(--fs-sm);
    color: var(--muted);
  }
</style>
