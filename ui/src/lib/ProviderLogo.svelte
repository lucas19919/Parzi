<script lang="ts">
  import { PROVIDER_MARKS } from "./providerMarks";

  /** Provider id, e.g. "claude", "codex", "xai". Unknown ids render nothing. */
  export let provider: string;
  export let size = 14;
  /** Dimmed ink for secondary placements (thread rows, overviews). */
  export let muted = false;

  $: mark = PROVIDER_MARKS[provider];
</script>

{#if mark}
  <svg
    width={size} height={size} viewBox={mark.viewBox} aria-hidden="true"
    class="mark" class:muted fill={mark.fill} fill-rule={mark.fillRule}
  >{@html mark.body}</svg>
{/if}

<style>
  .mark { display: block; flex: none; }
  .mark.muted { opacity: 0.8; }
</style>
