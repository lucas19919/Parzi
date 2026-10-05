<script lang="ts">
  import { PROVIDER_MARKS } from "./providerMarks";

  export let provider: string;
  export let size = 14;

  $: mark = PROVIDER_MARKS[provider];
</script>

{#if mark}
  <svg
    width={size} height={size} viewBox={mark.viewBox} aria-hidden="true"
    class="mark" fill={mark.fill} fill-rule={mark.fillRule}
  >{@html mark.body}</svg>
{/if}

<style>
  .mark { display: block; flex: none; }
</style>
