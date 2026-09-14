<script lang="ts">
  import { createEventDispatcher } from "svelte";
  export let on = false;
  export let title = "";
  const dispatch = createEventDispatcher<{ toggle: void }>();
</script>

<button
  class="switch"
  class:on
  role="switch"
  aria-checked={on}
  {title}
  on:click={() => dispatch("toggle")}
><span class="knob" /></button>
