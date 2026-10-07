<script lang="ts">
  import { createEventDispatcher } from "svelte";

  export let label = "";
  export let value = 0;
  export let min = 0;
  export let max = 100;
  export let step = 1;
  export let unit = "";

  const dispatch = createEventDispatcher<{ input: number }>();

  $: pct = max > min ? Math.min(100, Math.max(0, ((value - min) / (max - min)) * 100)) : 0;
  $: shown = Number.isInteger(step) ? String(Math.round(value)) : String(value);

  function onRange(e: Event) {
    value = +(e.currentTarget as HTMLInputElement).value;
    dispatch("input", value);
  }
</script>

<div class="row">
  <div class="info">
    <span class="lab">{label}</span>
  </div>
  <div class="ctl">
    <input type="range" {min} {max} {step} value={value} style="--p:{pct}%" aria-label={label} on:input={onRange} />
    <span class="val">{shown}{unit}</span>
  </div>
</div>

<style>
  .row { display: grid; grid-template-columns: 132px 1fr; align-items: center; gap: 14px; }
  .info { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .lab { font-size: 12.5px; font-weight: 500; color: var(--text); }
  .ctl { display: flex; align-items: center; gap: 12px; }
  input[type="range"] {
    -webkit-appearance: none; appearance: none;
    flex: 1; height: 4px; margin: 0; border-radius: 999px; outline: none; cursor: pointer;
    background: linear-gradient(90deg, var(--accent) var(--p), var(--line) var(--p));
  }
  input[type="range"]::-webkit-slider-thumb {
    -webkit-appearance: none; width: 14px; height: 14px; border-radius: 50%;
    background: var(--text); border: none; box-shadow: 0 1px 4px rgba(0, 0, 0, 0.4);
    transition: transform 140ms ease;
  }
  input[type="range"]:hover::-webkit-slider-thumb,
  input[type="range"]:active::-webkit-slider-thumb { transform: scale(1.15); }
  input[type="range"]:focus-visible { outline: 2px solid var(--accent); outline-offset: 4px; }
  .val {
    font-family: var(--mono), ui-monospace, monospace; font-size: 11px; color: var(--muted);
    width: 44px; text-align: right; font-variant-numeric: tabular-nums;
  }
</style>
