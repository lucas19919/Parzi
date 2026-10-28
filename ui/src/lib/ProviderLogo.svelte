<script lang="ts">
  import { PROVIDER_MARKS } from "./providerMarks";

  export let provider: string;
  export let size = 14;
  // The tab strip wants the mark in the icon color, not the brand paint.
  export let mono = false;

  $: mark = PROVIDER_MARKS[provider];
  $: body = mark && mono ? mark.body.replace(/fill="(?!#(?:fff|ffffff)\b)[^"]*"/gi, 'fill="currentColor"') : mark?.body;
</script>

{#if mark}
  <svg
    width={size} height={size} viewBox={mark.viewBox} aria-hidden="true"
    class="mark" fill={mono ? "currentColor" : mark.fill} fill-rule={mark.fillRule}
  >{@html body}</svg>
{/if}

<style>
  .mark { display: block; flex: none; }
</style>
