<script lang="ts" context="module">
  // The roster rules are pure and unit-tested, so they live next door in
  // `roster.ts`; this block only re-exports them for the wizard's import.
  export { candidates, defaultRoster } from "./roster";
</script>

<script lang="ts">
  import type { ModelRow } from "../api";
  import ModelPicker from "../ModelPicker.svelte";

  export let label = "";
  export let rows: ModelRow[] = [];
  export let value = "";
</script>

<div class="role">
  <div class="lab">{label}</div>
  <ModelPicker {label} models={rows} bind:value />
</div>

<style>
  .role { display: flex; flex-direction: column; gap: 4px; min-width: 0; }
  .lab { font-size: 12px; font-weight: 600; color: var(--text-3); }
</style>
