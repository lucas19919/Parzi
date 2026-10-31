<script lang="ts">
  import { ICONS, type IconName } from "./icons";

  export let name: IconName;
  export let size: 12 | 16 = 16;

  // One pixel at either size, the same weight as the window controls.
  $: weight = 24 / size;
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={weight}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {@html ICONS[name]}
</svg>

<style>
  svg {
    flex: none;
  }
</style>
