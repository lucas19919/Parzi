<script lang="ts">
  // Charts, tables and lists from the old `parzi-widget` blocks, which saved
  // sessions still hold. They draw with the same viz components as a `viz`
  // block, so there is one chart style in the app. A markdown widget is text.
  import { mdHtml, richReady } from "../md";
  import { handleLinkClick } from "../links";
  import Viz from "../viz/Viz.svelte";

  export let data: unknown;

  $: d = data && typeof data === "object" ? (data as Record<string, unknown>) : {};
  $: text = d.type === "markdown" ? String(d.text ?? d.title ?? "") : "";
</script>

{#if text}
  <!-- svelte-ignore a11y-no-static-element-interactions a11y-click-events-have-key-events -->
  <div class="msg" on:click={(e) => void handleLinkClick(e)}>{@html mdHtml(text, $richReady)}</div>
{:else}
  <Viz spec={data} />
{/if}
