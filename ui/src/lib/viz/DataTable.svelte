<script lang="ts">
  // Every value a chart draws, as a table only screen readers see: the
  // tooltip is for pointers, this is the same data as text.
  export let caption = "";
  export let head: string[] = [];
  export let rows: string[][] = [];
</script>

<table class="viz-data">
  {#if caption}<caption>{caption}</caption>{/if}
  <thead><tr>{#each head as h}<th scope="col">{h}</th>{/each}</tr></thead>
  <tbody>
    {#each rows as r}<tr>{#each r as c, i}{#if i === 0}<th scope="row">{c}</th>{:else}<td>{c}</td>{/if}{/each}</tr>{/each}
  </tbody>
</table>

<style>
  .viz-data {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    padding: 0;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
    border: 0;
  }
  td {
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
</style>
