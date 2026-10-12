<script lang="ts">
  import { createEventDispatcher } from "svelte";
  export let options: { value: string; label: string }[] = [];
  export let value = "";
  export let label = "";
  const dispatch = createEventDispatcher<{ pick: string }>();
</script>

<div class="segmented-control" role="group" aria-label={label || undefined} data-ui-segmented>
  {#each options as o}
    <button
      class="seg-btn"
      class:active={o.value === value}
      aria-pressed={o.value === value}
      on:click={() => dispatch("pick", o.value)}
    >{o.label}</button>
  {/each}
</div>
