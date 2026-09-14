<script lang="ts">
  import { createEventDispatcher } from "svelte";
  export let options: { value: string; label: string }[] = [];
  export let value = "";
  const dispatch = createEventDispatcher<{ pick: string }>();
</script>

<div class="segmented-control">
  {#each options as o}
    <button
      class="seg-btn"
      class:active={o.value === value}
      on:click={() => dispatch("pick", o.value)}
    >{o.label}</button>
  {/each}
</div>
