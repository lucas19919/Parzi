<script lang="ts">
  import { createEventDispatcher } from "svelte";
  export let on = false;
  export let title = "";
  export let disabled = false;
  const dispatch = createEventDispatcher<{ toggle: void }>();
</script>

<button
  class="switch"
  class:on
  class:locked={disabled}
  role="switch"
  aria-checked={on}
  aria-disabled={disabled}
  {title}
  disabled={disabled}
  on:click={() => dispatch("toggle")}
><span class="knob" /></button>
