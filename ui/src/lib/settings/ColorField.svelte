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
  <span class="lab">{label}</span>
  <span class="sw" style="background:{value}">
    <input type="color" value={isHex(value) ? normalizeHex(value) : "#000000"} on:input={fromPicker} aria-label="{label} colour" />
  </span>
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
  .cf {
    display: flex;
    align-items: center;
    gap: calc(var(--space) * 3);
    min-width: 0;
  }
  .sw {
    position: relative;
    flex: none;
    width: 28px;
    height: 28px;
    overflow: hidden;
    border-radius: 50%;
    box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.25), 0 0 0 1px var(--line);
    cursor: pointer;
  }
  .sw input {
    position: absolute;
    inset: -10px;
    width: 44px;
    height: 44px;
    padding: 0;
    border: none;
    opacity: 0;
    cursor: pointer;
  }
  .lab {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--text);
    font-size: var(--fs-md);
    font-weight: 500;
  }
  /* The same box as the font fields: the control column, one field tall. */
  .hex {
    flex: none;
    box-sizing: border-box;
    width: var(--w-ctl);
    height: var(--h-field);
    padding: 0 calc(var(--space) * 2);
    background: var(--bg);
    border: 1px solid var(--line);
    border-radius: var(--radius);
    outline: none;
    color: var(--muted);
    font-family: var(--mono), ui-monospace, monospace;
    font-size: var(--fs-sm);
    text-transform: uppercase;
  }
  .hex:focus {
    border-color: var(--accent);
    box-shadow: none;
    color: var(--text);
  }
</style>
