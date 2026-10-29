<script lang="ts">
  export let caption = "";
  export let head: string[] = [];
  export let rows: string[][] = [];
</script>


<div class="viz-data">
  <table>
    {#if caption}<caption>{caption}</caption>{/if}
    <thead><tr>{#each head as h}<th scope="col">{h}</th>{/each}</tr></thead>
    <tbody>
      {#each rows as r}<tr>{#each r as c, i}{#if i === 0}<th scope="row">{c}</th>{:else}<td>{c}</td>{/if}{/each}</tr>{/each}
    </tbody>
  </table>
</div>

<style>
  .viz-data {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: calc(var(--space) * -1);
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
