<script lang="ts">
  import { createEventDispatcher } from "svelte";
  import { isHex, normalizeHex } from "../theme";

  export let label = "";
  export let value = "#000000";

  const dispatch = createEventDispatcher<{ input: string }>();

  let draft = value;
  $: draft = value;

  function fromPicker(e: Event) {
    dispatch("input", normalizeHex((e.currentTarget as HTMLInputElement).value));
  }
  function fromText() {
    const v = draft.trim();
    if (isHex(v)) dispatch("input", normalizeHex(v));
    else draft = value;
  }
</script>

<label class="cf">
  <span class="sw" style="background:{value}">
    <input type="color" value={isHex(value) ? normalizeHex(value) : "#000000"} on:input={fromPicker} aria-label="{label} colour" />
  </span>
  <span class="lab">{label}</span>
  <input
    class="hex"
    bind:value={draft}
    on:change={fromText}
    on:keydown={(e) => e.key === "Enter" && fromText()}
    spellcheck="false"
    aria-label="{label} hex"
  />
</label>

<style>
  .cf { display: flex; align-items: center; gap: 10px; min-width: 0; }
  .sw {
    position: relative; width: 24px; height: 24px; flex: none; border-radius: 50%; overflow: hidden;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.25), 0 0 0 1px var(--line-3); cursor: pointer;
  }
  .sw input { position: absolute; inset: -10px; width: 44px; height: 44px; opacity: 0; cursor: pointer; padding: 0; border: none; }
  .lab { flex: 1; min-width: 0; font-size: 12.5px; color: var(--text-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .hex {
    width: 84px; flex: none; background: var(--input); border: 1px solid var(--line-2);
    border-radius: var(--radius-1); color: var(--text-2); padding: 4px 7px; outline: none;
    font-family: var(--parzi-mono), ui-monospace, monospace; font-size: 11px; text-transform: uppercase;
  }
  .hex:focus { border-color: var(--accent-line); color: var(--text); box-shadow: none; }
</style>
