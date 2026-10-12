<script lang="ts">
  import { ICONS, type IconName } from "./icons";

  export let name: IconName;
  // 16 for controls, 12 for small inline glyphs. Octicons are drawn on these
  // grids, so their edges land on whole pixels; other sizes would blur.
  export let size: 12 | 16 = 16;

  $: icon = ICONS[name];
  $: grid = size === 12 && icon[12] ? 12 : 16;
</script>

<svg width={size} height={size} viewBox="0 0 {grid} {grid}" fill="currentColor" aria-hidden="true">
  {@html grid === 12 ? icon[12] : icon[16]}
</svg>

<style>
  svg {
    flex: none;
  }
</style>
