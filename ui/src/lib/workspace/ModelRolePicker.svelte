<script lang="ts">
  import type { ProviderStatus } from "../api";
  import ModelPicker from "../ModelPicker.svelte";

  export let label = "";
  export let board: ProviderStatus[] = [];
  export let value = "";
</script>

<div class="role">
  <div class="lab">{label}</div>
  <ModelPicker {label} {board} bind:value />
</div>

<style>
  .role { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .lab { font-size: 12px; font-weight: 600; color: var(--text-3); }
</style>
