<script lang="ts">
  const SCALE = [11, 12, 13, 14, 16, 18, 22];
  const letterSize = (badge: number) => SCALE.reduce((best, n) => (Math.abs(n - badge * 0.42) < Math.abs(best - badge * 0.42) ? n : best), SCALE[0]);
  import { CONNECTOR_MARKS } from "./connectorMarks";

  export let id = "";
  export let name = "";
  export let site = "";
  export let size = 32;
  export let src: string | null = null;

  let broken = false;

  $: mark = CONNECTOR_MARKS[id];
  $: dark = mark ? luma(mark.hex) < 0.25 : false;
  $: icon = site ? `${site.replace(/\/+$/, "")}/favicon.ico` : "";

  function luma(hex: string): number {
    const n = parseInt(hex.replace("#", ""), 16);
    const [r, g, b] = [(n >> 16) & 255, (n >> 8) & 255, n & 255];
    return (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255;
  }
</script>

<span class="logo" style:width="{size}px" style:height="{size}px" style:--brand={mark && !dark ? mark.hex : "var(--text)"} aria-hidden="true">
  {#if src}
    <img class="own" {src} alt="" width={Math.round(size * 0.62)} height={Math.round(size * 0.62)} />
  {:else if mark}
    <svg viewBox="0 0 24 24" width={Math.round(size * 0.56)} height={Math.round(size * 0.56)}><path d={mark.path} fill="currentColor" /></svg>
  {:else if icon && !broken}
    <img src={icon} alt="" width={Math.round(size * 0.56)} height={Math.round(size * 0.56)} on:error={() => (broken = true)} />
  {:else}
    <span class="letter" style:font-size="{letterSize(size)}px">{(name || id).slice(0, 1).toUpperCase()}</span>
  {/if}
</span>

<style>
  .logo {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 25%;
    background: color-mix(in srgb, var(--brand) 14%, var(--panel));
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--brand) 22%, transparent);
    color: var(--brand);
  }
  img {
    border-radius: var(--radius-sm);
    object-fit: contain;
  }
  .letter {
    font-weight: 650;
    color: var(--text);
  }
</style>
